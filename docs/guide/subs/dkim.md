# Configuring DKIM

## What it is

DomainKeys Identified Mail (DKIM) is a mechanism that allows verification of the source and contents of email messages. Using DKIM, sending domains can include a cryptographic signature in outgoing email messages. A message's signature may be verified by an MTA during transit and by the Mail User Agent (MUA) upon delivery. A verified signature indicates the message was sent by the sending domain and the message was not altered in transit. When a DKIM signature fails verification that indicates the message may have been altered during transit or that the sender is fraudulently using the sending domain name. The DKIM specification is located here: [rfc6376](https://datatracker.ietf.org/doc/html/rfc6376).

From the RFC ([section 6.2](https://datatracker.ietf.org/doc/html/rfc6376#section-6.2)):
_"Verifiers wishing to communicate the results of verification to other
   parts of the mail system may do so in whatever manner they see fit.
   For example, implementations might choose to add an email header
   field to the message before passing it on.  Any such header field
   SHOULD be inserted before any existing DKIM-Signature or preexisting
   authentication status header fields in the header field block.  The
   Authentication-Results: header field (RFC5451) MAY be used for this
   purpose."_


This diagram gives a graphical view of how DKIM works.

![Diagram showing DNS being used to store and validate DKIM keys](https://user-images.githubusercontent.com/13574506/222079787-83431e17-5737-42dc-9cbf-9679477718c9.png)



## For Sending Servers
*Set up*

A systenm administrator with access to manage DNS generates a public/private key pair to use for signing all outgoing messages for the domain (multiple key pairs are allowed). The public key is published in DNS, and the private key is made available to their DKIM-enabled outbound email servers. This is step "1" in the diagram.

*Signing*

When an email is sent by an authorized user within the domain, the DKIM-enabled email system uses the stored private key to generate a digital signature of the message. This signature is included in a DKIM-Signature header and prepended to the email. The email is then sent on to the recipient's mail server. This is step "2" in the diagram.

## For Receiving Servers
*Preparation*

The DKIM-enabled receiving email system extracts and parses the message's DKIM-Signature header. The signing domain asserted by the header is used to fetch the signer's public key from DNS. This is step "3" in the diagram.

*Verification*

The signer's public key is then used by the receiving mail system to verify that the signature contained in the DKIM-Signature header was generated by the sending domain's private key. This proves that the email was truly sent by, and with the permission of, the claimed sending domain. It also confirms that the headers signed by the sending domain and the message body were not altered during transit.

*Delivery*

The receiving email system uses the outcome of signature verification along with other local policies and tests to determine the disposition of the message. If local policy does not prohibit delivery, the message is passed to the user's inbox. Optionally, the email recipient may be informed of the results of the signature verification. This is step "4" in the diagram.

## DKIM Signing
Enabling DKIM signing in KumoMTA:

Generate public and private keys for each signing domain and create the DKIM public key DNS records for those domains.

Generating DKIM Keys#
The OpenSSL cryptography toolkit can be used to generate RSA keys for DKIM. As an example, the following openssl commands are used to generate public and private keys for the domain example.com with a selector called "default". The files can be stored in any directory such as ~/kumomta/keys/.

```
mkdir -p ~/kumomta/keys/example.com
openssl genrsa -out ~/kumomta/keys/example.com/default.key 1024
openssl rsa -in ~/kumomta/keys/example.com/default.key -out ~/kumomta/keys/example.com/default.pub -pubout -outform PEM
```

Any DKIM verification implementations must support key sizes of 512, 768, 1024, 1536, and 2048 bits. A signer may choose to sign messages using any of these sizes and may use a different size for different selectors. Larger key sizes provide greater security but impose higher CPU costs during message signing and verification. It is not recommended to use a key size lower than 1024 unless absolutely necessary. Note that Google _requires_ senders to sign with a 1024 bit or greater key size.

The resulting public key should look similar to:

```
-----BEGIN PUBLIC KEY-----
MIGKSINEN09U6CSqGSIb3DQEBAQUAA4GNADCBiQKBgQCrZXNwzXOk0mRqPcgSUOF
rg/BZHybpiBoDS/g6IaMjmVwaQf2E72x9yDBTgiUBtDIWN76KQRZJ3Ebfvo+WAHq
2yz6HKR0XCwMDSE2S3brVe7mbV/GPEvnPEVjbfL4w0tEjahuBN9kty7h07uVQqy1
Q7jIOnF5fG9AQNd1UyTvd43QAB
-----END PUBLIC KEY-------
```

Once the public and private keys have been generated, create a DNS text record for default._domainkey.example.com. The DNS record contains several DKIM "tag=value" pairs and should be similiar to the record shown below:

default._domainkey.example.com. 86400 IN TXT
"v=DKIM1; k=rsa; h=sha256; t=y; p=MHww...QAB"

DKIM DNS text record tags are defined below. Do not include the quotes below when including a tag value in the DNS text record.

v= DKIM key record version. The value of this tag must be set to "DKIM1".

k= Key type. This tag defines the syntax and semantics of the p= tag value. Currently, this tag should have the value "rsa".

h= Hash algorithm. Currently, this tag should have the value "sha1" or "sha256".

t= Flags. The only value currently defined is "y". If specified, this tag indicates the signing domain is testing DKIM.

p= DKIM public key value generated as described above.

s= Service Type. If specified, this tag should be set to "*" or "email" which represents all service types or the email service type. Currently, "email" is the only service using this key.

n= Notes. If specified, the value of this tag is quoted-printable text used as a note to anyone reading the DNS text record. The tag is not interpreted by DKIM verification and should be used sparingly because of space limitations of the DNS text record.

## Implement the signing process

Configure KumoMTA to sign emails passing through the MTA with DKIM signatures.  This is done with Lua in policy.  The simple_policy.lua policy provided with KumoMTA declairs a basic working DKIM signer that you can copy and modify as needed.  Tis signs a message with RSA256 using a selector "default" on headers 'From', 'To', and 'Subject' using the DKIM key located at example-private-dkim-key.pem. 

  local signer = kumo.dkim.rsa_sha256_signer {
    domain = msg:sender().domain,
    selector = 'default',
    headers = { 'From', 'To', 'Subject' },
    key = 'example-private-dkim-key.pem',
  }

Where you want to enable dkim signing, simple call that signer in policy.
IE:  msg:dkim_sign(signer)



### EOF
