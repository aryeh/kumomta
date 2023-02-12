use crate::lua_config::{load_config, LuaConfig};
use anyhow::anyhow;
use message::{EnvelopeAddress, Message};
use std::fmt::Debug;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter, ReadHalf,
    WriteHalf,
};
use tracing::{error, instrument};

#[derive(Debug)]
pub struct SmtpServer<T> {
    reader: BufReader<ReadHalf<T>>,
    writer: BufWriter<WriteHalf<T>>,
    state: Option<TransactionState>,
    said_hello: Option<String>,
    config: LuaConfig,
}

#[derive(Debug)]
struct TransactionState {
    sender: EnvelopeAddress,
    recipients: Vec<EnvelopeAddress>,
    data: Vec<u8>,
    meta: serde_json::Value,
}

impl<T: AsyncRead + AsyncWrite + Debug + Send + 'static> SmtpServer<T> {
    pub async fn run(socket: T) -> anyhow::Result<()> {
        let config = load_config().await?;
        let (reader, writer) = tokio::io::split(socket);
        let reader = tokio::io::BufReader::new(reader);
        let writer = tokio::io::BufWriter::new(writer);
        let mut server = SmtpServer {
            reader,
            writer,
            state: None,
            said_hello: None,
            config,
        };

        tokio::spawn(async move {
            if let Err(err) = server.process().await {
                error!("Error in SmtpServer: {err:#}");
                server
                    .write_response(421, "technical difficulties")
                    .await
                    .ok();
            }
        });
        Ok(())
    }

    async fn write_response<S: AsRef<str>>(
        &mut self,
        status: u16,
        message: S,
    ) -> anyhow::Result<()> {
        let mut lines = message.as_ref().lines().peekable();
        while let Some(line) = lines.next() {
            let is_last = lines.peek().is_none();
            let sep = if is_last { ' ' } else { '-' };
            let text = format!("{status}{sep}{line}\r\n");
            self.writer.write(text.as_bytes()).await?;
        }
        self.writer.flush().await?;
        Ok(())
    }

    async fn read_line(&mut self) -> anyhow::Result<String> {
        let mut line = String::new();
        self.reader.read_line(&mut line).await?;
        Ok(line)
    }

    #[instrument(skip(self))]
    async fn process(&mut self) -> anyhow::Result<()> {
        self.write_response(220, "mail.example.com KumoMTA\nW00t!\nYeah!")
            .await?;
        loop {
            let line = self.read_line().await?;
            let line = line.trim_end();

            match Command::parse(line) {
                Err(err) => {
                    self.write_response(
                        501,
                        format!("Syntax error in command or arguments: {err}"),
                    )
                    .await?;
                }
                Ok(Command::Quit) => {
                    self.write_response(221, "So long, and thanks for all the fish!")
                        .await?;
                    return Ok(());
                }
                Ok(Command::Ehlo(domain)) => {
                    // TODO: we are supposed to report extension commands in our EHLO
                    // response, but we don't have any yet.

                    self.config
                        .call_callback("smtp_server_ehlo", domain.clone())?;

                    self.write_response(250, format!("mail.example.com Hello {domain}"))
                        .await?;
                    self.said_hello.replace(domain);
                }
                Ok(Command::Helo(domain)) => {
                    self.config
                        .call_callback("smtp_server_ehlo", domain.clone())?;
                    self.write_response(250, format!("Hello {domain}!")).await?;
                    self.said_hello.replace(domain);
                }
                Ok(Command::Mail(address)) => {
                    if self.state.is_some() {
                        self.write_response(503, "MAIL FROM already issued; you must RSET first")
                            .await?;
                        continue;
                    }
                    self.config
                        .call_callback("smtp_server_mail_from", address.clone())?;
                    self.state.replace(TransactionState {
                        sender: address.clone(),
                        recipients: vec![],
                        data: vec![],
                        meta: serde_json::json!({}),
                    });
                    self.write_response(250, format!("OK {address:?}")).await?;
                }
                Ok(Command::Rcpt(address)) => {
                    if self.state.is_none() {
                        self.write_response(503, "MAIL FROM must be issued first")
                            .await?;
                        continue;
                    }
                    self.config
                        .call_callback("smtp_server_mail_rcpt_to", address.clone())?;
                    self.write_response(250, format!("OK {address:?}")).await?;
                    self.state
                        .as_mut()
                        .expect("checked state above")
                        .recipients
                        .push(address);
                }
                Ok(Command::Data) => {
                    if self.state.is_none() {
                        self.write_response(503, "MAIL FROM must be issued first")
                            .await?;
                        continue;
                    }
                    if self
                        .state
                        .as_ref()
                        .map(|s| s.recipients.is_empty())
                        .unwrap_or(true)
                    {
                        self.write_response(503, "RCPT TO must be issued first")
                            .await?;
                        continue;
                    }
                    self.write_response(354, "Send body; end with CRLF.CRLF")
                        .await?;

                    let mut data = vec![];

                    loop {
                        let line = self.read_line().await?;
                        if line == ".\r\n" {
                            break;
                        }

                        let line = if line.starts_with('.') {
                            &line[1..]
                        } else {
                            &line
                        };

                        data.extend_from_slice(line.as_bytes());
                    }

                    let mut state = self
                        .state
                        .take()
                        .ok_or_else(|| anyhow!("transaction state is impossibly not set!?"))?;

                    state.data = data;

                    tracing::trace!(?state);

                    let mut ids = vec![];
                    for recip in state.recipients {
                        let message = Message::new_dirty(
                            state.sender.clone(),
                            recip,
                            state.meta.clone(),
                            state.data.clone(),
                        )?;

                        ids.push(message.id().await.to_string());

                        self.config
                            .call_callback("smtp_server_message_received", message.clone())?;
                    }

                    let ids = ids.join(" ");
                    self.write_response(250, format!("OK ids={ids}")).await?;
                }
                Ok(Command::Rset) => {
                    self.state.take();
                    self.write_response(250, "Reset state").await?;
                }
                Ok(Command::Noop) => {
                    self.write_response(250, "the goggles do nothing").await?;
                }
                Ok(Command::Unknown(cmd)) => {
                    self.write_response(502, format!("Command unrecognized/unimplemented: {cmd}"))
                        .await?;
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Command {
    Ehlo(String),
    Helo(String),
    Mail(EnvelopeAddress),
    Rcpt(EnvelopeAddress),
    Data,
    Rset,
    Noop,
    Quit,
    Unknown(String),
}

impl Command {
    fn parse(line: &str) -> anyhow::Result<Self> {
        fn prefix_match(line: &str, candidate: &str) -> bool {
            if line.len() < candidate.len() {
                false
            } else {
                line[..candidate.len()].eq_ignore_ascii_case(candidate)
            }
        }

        fn extract_envelope(line: &str) -> anyhow::Result<(&str, &str)> {
            if !line.starts_with('<') {
                anyhow::bail!("expected <: {line:?}");
            }
            let rangle = line
                .bytes()
                .position(|c| c == b'>')
                .ok_or_else(|| anyhow::anyhow!("expected >: {line:?}"))?;

            Ok((&line[1..rangle], &line[rangle + 1..]))
        }

        Ok(if line.eq_ignore_ascii_case("QUIT") {
            Self::Quit
        } else if line.eq_ignore_ascii_case("DATA") {
            Self::Data
        } else if line.eq_ignore_ascii_case("RSET") {
            Self::Rset
        } else if line.eq_ignore_ascii_case("NOOP") {
            Self::Noop
        } else if prefix_match(line, "EHLO ") {
            Self::Ehlo(line[5..].to_string())
        } else if prefix_match(line, "HELO ") {
            Self::Helo(line[5..].to_string())
        } else if prefix_match(line, "MAIL FROM:") {
            let (address, _params) = extract_envelope(&line[10..])?;
            // TODO: MAIL FROM can accept key/value parameters
            Self::Mail(EnvelopeAddress::parse(address)?)
        } else if prefix_match(line, "RCPT TO:") {
            let (address, _params) = extract_envelope(&line[8..])?;
            if address.is_empty() {
                anyhow::bail!("Null sender not permitted as a recipient");
            }
            Self::Rcpt(EnvelopeAddress::parse(address)?)
        } else {
            Self::Unknown(line.to_string())
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use k9::assert_equal;

    #[test]
    fn command_parser() {
        assert_equal!(Command::parse("QUIT").unwrap(), Command::Quit);
        assert_equal!(Command::parse("quit").unwrap(), Command::Quit);
        assert_equal!(
            Command::parse("quite").unwrap(),
            Command::Unknown("quite".to_string())
        );
        assert_equal!(
            Command::parse("flibble").unwrap(),
            Command::Unknown("flibble".to_string())
        );
        assert_equal!(
            Command::parse("MAIL From:<>").unwrap(),
            Command::Mail(EnvelopeAddress::null_sender())
        );
        assert_equal!(
            Command::parse("MAIL From:<user@example.com>").unwrap(),
            Command::Mail(EnvelopeAddress::parse("user@example.com").unwrap())
        );
        assert_equal!(
            Command::parse("rcpt to:<>").unwrap_err().to_string(),
            "Null sender not permitted as a recipient"
        );
        assert_equal!(
            Command::parse("rcpt TO:<user@example.com>").unwrap(),
            Command::Rcpt(EnvelopeAddress::parse("user@example.com").unwrap())
        );
    }
}
