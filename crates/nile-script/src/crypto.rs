//! The cryptography behind `crypto.subtle` (web.js `/*@CRYPTO@*/`, crypto.js): digests, HMAC, PBKDF2, AES-GCM and
//! random bytes over `ring`. One entry point, `call(op, args)`; bytes travel as base64 text, results are base64 (or
//! `"true"`/`"false"`), errors are short messages.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ring::{aead, digest, hmac, pbkdf2, rand::{SecureRandom, SystemRandom}};
use std::num::NonZeroU32;

fn bytes(args: &[String], i: usize) -> Result<Vec<u8>, String> {
    B64.decode(args.get(i).map_or("", String::as_str)).map_err(|_| "invalid data".to_string())
}

fn algorithm(name: &str) -> Result<&'static digest::Algorithm, String> {
    match name.to_ascii_uppercase().as_str() {
        "SHA-1" => Ok(&digest::SHA1_FOR_LEGACY_USE_ONLY),
        "SHA-256" => Ok(&digest::SHA256),
        "SHA-384" => Ok(&digest::SHA384),
        "SHA-512" => Ok(&digest::SHA512),
        other => Err(format!("unsupported hash: {other}")),
    }
}

fn hmac_algorithm(name: &str) -> Result<hmac::Algorithm, String> {
    match name.to_ascii_uppercase().as_str() {
        "SHA-1" => Ok(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY),
        "SHA-256" => Ok(hmac::HMAC_SHA256),
        "SHA-384" => Ok(hmac::HMAC_SHA384),
        "SHA-512" => Ok(hmac::HMAC_SHA512),
        other => Err(format!("unsupported hash: {other}")),
    }
}

fn pbkdf2_algorithm(name: &str) -> Result<pbkdf2::Algorithm, String> {
    match name.to_ascii_uppercase().as_str() {
        "SHA-1" => Ok(pbkdf2::PBKDF2_HMAC_SHA1),
        "SHA-256" => Ok(pbkdf2::PBKDF2_HMAC_SHA256),
        "SHA-384" => Ok(pbkdf2::PBKDF2_HMAC_SHA384),
        "SHA-512" => Ok(pbkdf2::PBKDF2_HMAC_SHA512),
        other => Err(format!("unsupported hash: {other}")),
    }
}

fn aes_key(raw: &[u8]) -> Result<aead::LessSafeKey, String> {
    let alg = match raw.len() {
        16 => &aead::AES_128_GCM,
        32 => &aead::AES_256_GCM,
        n => return Err(format!("AES-GCM keys are 128 or 256 bits, not {}", n * 8)),
    };
    aead::UnboundKey::new(alg, raw).map(aead::LessSafeKey::new).map_err(|_| "invalid key".to_string())
}

fn nonce(iv: &[u8]) -> Result<aead::Nonce, String> {
    aead::Nonce::try_assume_unique_for_key(iv).map_err(|_| "AES-GCM needs a 12 byte iv".to_string())
}

pub fn call(op: &str, args: &[String]) -> Result<String, String> {
    let arg = |i: usize| args.get(i).map_or("", String::as_str);
    match op {
        "random" => {
            let n: usize = arg(0).parse().map_err(|_| "invalid length".to_string())?;
            if n > 65536 {
                return Err("QuotaExceededError: at most 65536 bytes at a time".into());
            }
            let mut out = vec![0u8; n];
            SystemRandom::new().fill(&mut out).map_err(|_| "no random source".to_string())?;
            Ok(B64.encode(out))
        }
        "digest" => Ok(B64.encode(digest::digest(algorithm(arg(0))?, &bytes(args, 1)?).as_ref())),
        "hmac" => {
            let key = hmac::Key::new(hmac_algorithm(arg(0))?, &bytes(args, 1)?);
            Ok(B64.encode(hmac::sign(&key, &bytes(args, 2)?).as_ref()))
        }
        // `ring` compares in constant time.
        "hmac_verify" => {
            let key = hmac::Key::new(hmac_algorithm(arg(0))?, &bytes(args, 1)?);
            Ok(hmac::verify(&key, &bytes(args, 2)?, &bytes(args, 3)?).is_ok().to_string())
        }
        "pbkdf2" => {
            let alg = pbkdf2_algorithm(arg(0))?;
            let iterations = arg(3).parse::<u32>().ok().and_then(NonZeroU32::new).ok_or("iterations must be at least 1")?;
            let bits: usize = arg(4).parse().map_err(|_| "invalid length".to_string())?;
            if bits == 0 || !bits.is_multiple_of(8) || bits > 8 * 1024 {
                return Err("length must be a multiple of 8 bits, at most 8192".into());
            }
            let mut out = vec![0u8; bits / 8];
            pbkdf2::derive(alg, iterations, &bytes(args, 2)?, &bytes(args, 1)?, &mut out);
            Ok(B64.encode(out))
        }
        // key, iv, data, aad -> ciphertext followed by the 16 byte tag
        "aes_gcm_seal" => {
            let key = aes_key(&bytes(args, 0)?)?;
            let mut buf = bytes(args, 2)?;
            key.seal_in_place_append_tag(nonce(&bytes(args, 1)?)?, aead::Aad::from(bytes(args, 3)?), &mut buf).map_err(|_| "encryption failed".to_string())?;
            Ok(B64.encode(buf))
        }
        "aes_gcm_open" => {
            let key = aes_key(&bytes(args, 0)?)?;
            let mut buf = bytes(args, 2)?;
            let plain = key.open_in_place(nonce(&bytes(args, 1)?)?, aead::Aad::from(bytes(args, 3)?), &mut buf).map_err(|_| "OperationError: decryption failed (wrong key, or the data was changed)".to_string())?;
            Ok(B64.encode(plain))
        }
        other => Err(format!("unknown crypto operation: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(text: &str) -> String {
        B64.encode(text)
    }
    fn hex(b64: &str) -> String {
        B64.decode(b64).unwrap().iter().map(|x| format!("{x:02x}")).collect()
    }
    fn run(op: &str, args: &[&str]) -> String {
        call(op, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn digests_hmac_and_pbkdf2_match_the_published_vectors() {
        assert_eq!(hex(&run("digest", &["SHA-256", &b("abc")])), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&run("digest", &["sha-1", &b("abc")])), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert!(hex(&run("digest", &["SHA-512", &b("abc")])).starts_with("ddaf35a193617aba"));
        assert_eq!(hex(&run("hmac", &["SHA-256", &b("key"), &b("The quick brown fox jumps over the lazy dog")])), "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
        let tag = run("hmac", &["SHA-256", &b("key"), &b("data")]);
        assert_eq!(run("hmac_verify", &["SHA-256", &b("key"), &b("data"), &tag]), "true");
        assert_eq!(run("hmac_verify", &["SHA-256", &b("key"), &b("other"), &tag]), "false");
        // RFC 7914 / RFC 6070 style vectors for PBKDF2-HMAC-SHA256.
        assert_eq!(hex(&run("pbkdf2", &["SHA-256", &b("password"), &b("salt"), "1", "256"])), "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
        assert_eq!(hex(&run("pbkdf2", &["SHA-256", &b("password"), &b("salt"), "4096", "256"])), "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a");
        assert!(call("pbkdf2", &["SHA-256".into(), b("p"), b("s"), "0".into(), "256".into()]).is_err());
        assert!(call("digest", &["MD5".into(), b("x")]).is_err());
    }

    #[test]
    fn aes_gcm_matches_the_nist_vectors_and_rejects_tampering() {
        let key = B64.encode([0u8; 16]);
        let iv = B64.encode([0u8; 12]);
        // NIST GCM test cases 1 and 2.
        assert_eq!(hex(&run("aes_gcm_seal", &[&key, &iv, "", ""])), "58e2fccefa7e3061367f1d57a4e7455a");
        let sealed = run("aes_gcm_seal", &[&key, &iv, &B64.encode([0u8; 16]), ""]);
        assert_eq!(hex(&sealed), "0388dace60b6a392f328c2b971b2fe78ab6e47d42cec13bdf53a67b21257bddf");
        assert_eq!(hex(&run("aes_gcm_open", &[&key, &iv, &sealed, ""])), "00000000000000000000000000000000");
        let mut raw = B64.decode(&sealed).unwrap();
        raw[0] ^= 1;
        assert!(call("aes_gcm_open", &[key.clone(), iv.clone(), B64.encode(raw), String::new()]).is_err());
        assert!(call("aes_gcm_open", &[key, iv, sealed, b("other aad")]).is_err());
        assert!(call("aes_gcm_seal", &[B64.encode([0u8; 5]), B64.encode([0u8; 12]), String::new(), String::new()]).is_err());
    }

    #[test]
    fn random_bytes_have_the_asked_length_and_differ() {
        let a = run("random", &["32"]);
        let b = run("random", &["32"]);
        assert_eq!(B64.decode(&a).unwrap().len(), 32);
        assert_ne!(a, b);
        assert!(call("random", &["70000".into()]).is_err());
    }
}
