//! TLS path (ElkRP ConnectMethod 50) against a local TLS server with a self-signed certificate of the M1XEP issuer.

use elk_core::*;
use native_tls::{Identity, Protocol, TlsAcceptor};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn rsa_key_pem() -> &'static str {
    // Schannel can only import RSA keys, so the test server needs one (generated once per run).
    static KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    KEY.get_or_init(|| {
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        let k = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
        k.to_pkcs8_pem(LineEnding::LF).unwrap().to_string()
    })
}

fn issuer_cert(cn: &str) -> (Vec<u8>, String, String) {
    let mut p = CertificateParams::new(vec!["M1XEP".to_string()]).unwrap();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, cn);
    p.distinguished_name = dn;
    let key = KeyPair::from_pem(rsa_key_pem()).unwrap();
    let cert = p.self_signed(&key).unwrap();
    (cert.der().to_vec(), cert.pem(), rsa_key_pem().to_string())
}

#[test]
fn issuer_is_rendered_like_dotnet() {
    let (der, _, _) = issuer_cert("HTTPS Certificate");
    assert_eq!(Some("CN=HTTPS Certificate".to_string()), issuer_string(&der));

    let mut p = CertificateParams::new(vec!["M1XEP".to_string()]).unwrap();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CountryName, "US");
    dn.push(DnType::StateOrProvinceName, "NC");
    dn.push(DnType::LocalityName, "Hildebran");
    dn.push(DnType::OrganizationName, "Elk");
    dn.push(DnType::OrganizationalUnitName, "Engineering");
    dn.push(DnType::CommonName, "M1XEP");
    p.distinguished_name = dn;
    let key = KeyPair::generate().unwrap();
    let cert = p.self_signed(&key).unwrap();
    let got = issuer_string(cert.der()).unwrap();
    assert_eq!("CN=M1XEP, OU=Engineering, O=Elk, L=Hildebran, S=NC, C=US", got);
    assert!(TRUSTED_ISSUERS.contains(&got.as_str()));
    assert_eq!(None, issuer_string(&[1, 2, 3]));
}

/// A one-connection TLS "panel": answers the login with an accepted connect reply, then a clock-less echo of nothing.
fn serve_tls(cert_pem: String, key_pem: String) -> u16 {
    let identity = Identity::from_pkcs8(cert_pem.as_bytes(), key_pem.as_bytes()).unwrap();
    let acceptor = TlsAcceptor::builder(identity).min_protocol_version(Some(Protocol::Tlsv12)).build().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((tcp, _)) = listener.accept() {
            if let Ok(mut s) = acceptor.accept(tcp) {
                let mut parser = FrameParser::new(false);
                let mut buf = [0u8; 512];
                loop {
                    let n = match s.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    for f in parser.feed(&buf[..n]) {
                        if f.payload.first() == Some(&0x7F) && f.payload.len() == 26 {
                            // identity block present: accept
                            let mut r = vec![0u8; 25];
                            r[0] = 0x7F;
                            r[4] = 5;
                            r[5] = 3;
                            r[6] = 44;
                            r[8..12].copy_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                            let _ = s.write_all(&Framing::encode(&r));
                        }
                    }
                }
            }
        }
    });
    port
}

#[test]
fn tls_login_with_a_trusted_issuer_works() {
    let (_, cert, key) = issuer_cert("HTTPS Certificate");
    let port = serve_tls(cert, key);
    let mut s = ElkSession::tls("00000001").unwrap();
    s.connect("127.0.0.1", port).unwrap();
    let r = s.login("246801").unwrap();
    assert_eq!("5.3.44", r.firmware);
    assert_eq!("00000001", r.serial_hex);
}

#[test]
fn tls_with_an_untrusted_issuer_is_refused() {
    let (_, cert, key) = issuer_cert("Somebody Else");
    let port = serve_tls(cert, key);
    let mut s = ElkSession::tls("00000001").unwrap();
    let e = s.connect("127.0.0.1", port).unwrap_err();
    assert!(e.is_io());
    assert!(e.to_string().contains("untrusted certificate issuer CN=Somebody Else"), "{e}");
}
