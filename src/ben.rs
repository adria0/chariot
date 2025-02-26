use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::{collections::BTreeMap, fmt};

#[derive(Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ben {
    String(Vec<u8>),
    Number(i64),
    Array(Vec<Ben>),
    Dict(BTreeMap<Ben, Ben>),
}

impl fmt::Display for Ben {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            Ben::String(s) => {
                if let Ok(str) = String::from_utf8(s.to_vec()) {
                    write!(f, "\"")?;
                    write!(f, "{}", &str)?;
                    write!(f, "\"")
                } else {
                    write!(f, "0x")?;
                    write!(f, "{}", hex::encode(s))
                }
            }
            Ben::Number(n) => write!(f, "{}", &n.to_string()),
            Ben::Array(a) => {
                write!(f, "[")?;
                for (i, e) in a.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{}", e)?;
                }
                write!(f, "]")
            }
            Ben::Dict(d) => {
                write!(f, "{{")?;
                for (i, (k, v)) in d.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{}", k)?;
                    write!(f, ":")?;
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl Ben {
    pub fn mk_str(s: &str) -> Ben {
        Ben::String(s.as_bytes().to_vec())
    }
    pub fn mk_u32(n: u32) -> Ben {
        Ben::Number(n as i64)
    }
    pub fn mk_u8(n: u8) -> Ben {
        Ben::Number(n as i64)
    }
    pub fn mk_vec(v: &[Ben]) -> Ben {
        Ben::Array(v.to_vec())
    }
    pub fn mk_dict(d: &[(Ben, Ben)]) -> Ben {
        Ben::Dict(d.iter().cloned().collect())
    }
    pub fn str(&self) -> Result<String> {
        if let Ben::String(s) = self {
            Ok(String::from_utf8(s.to_vec())?)
        } else {
            bail!("cannot str a non-str")
        }
    }
    pub fn bytes(&self) -> Result<&[u8]> {
        if let Ben::String(s) = self {
            Ok(s)
        } else {
            bail!("cannot str a non-str")
        }
    }
    pub fn u32(&self) -> Result<u32> {
        if let Ben::Number(n) = self {
            Ok(*n as u32)
        } else {
            bail!("cannot u32 a non-number")
        }
    }
    pub fn array(&self, n: usize) -> Result<&Ben> {
        if let Ben::Array(a) = self {
            Ok(&a[n])
        } else {
            bail!("cannot index on non array")
        }
    }
    pub fn dict(&self, k: &str) -> Result<&Ben> {
        if let Ben::Dict(a) = self {
            Ok(a.get(&Ben::String(k.as_bytes().to_vec())).unwrap())
        } else {
            bail!("cannot index on non dict")
        }
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut r = Vec::new();
        self.encode_int(&mut r);
        r
    }

    fn encode_int(&self, encoded: &mut Vec<u8>) {
        match &self {
            Ben::String(s) => {
                encoded.append(&mut format!("{}:", s.len()).into_bytes());
                encoded.append(&mut s.clone())
            }
            Ben::Number(n) => encoded.append(&mut format!("i{}e", &n).into_bytes()),
            Ben::Array(a) => {
                encoded.push(b'l');
                for e in a {
                    e.encode_int(encoded);
                }
                encoded.push(b'e');
            }
            Ben::Dict(d) => {
                encoded.push(b'd');
                for (k, v) in d {
                    k.encode_int(encoded);
                    v.encode_int(encoded);
                }
                encoded.push(b'e');
            }
        }
    }
    pub fn decode(encoded_value: &[u8]) -> Result<Ben> {
        let (ben, len) = Self::decode_partial(encoded_value)?;
        assert_eq!(encoded_value.len(), len);
        Ok(ben)
    }
    pub fn decode_partial(encoded_value: &[u8]) -> Result<(Ben, usize)> {
        let prefix = *encoded_value.first().ok_or(anyhow!("no value"))?;

        match prefix {
            b'0'..=b'9' => {
                // Example: "5:hello" -> "hello"
                let colon_index = encoded_value
                    .iter()
                    .position(|c| *c == b':')
                    .ok_or(anyhow!("invalid string"))?;
                let number_string = &encoded_value[..colon_index];
                let number = String::from_utf8(number_string.to_vec())?.parse::<i64>()?;
                if colon_index + 1 + number as usize > encoded_value.len() {
                    bail!("invalid string");
                }
                let string = &encoded_value[colon_index + 1..colon_index + 1 + number as usize];
                Ok((
                    Ben::String(string.to_vec()),
                    number_string.len() + 1 + string.len(),
                ))
            }
            b'i' => {
                let integer_str =
                    if let Some(end_index) = encoded_value.iter().position(|ch| *ch == b'e') {
                        String::from_utf8(encoded_value[1..end_index].to_vec())?
                    } else {
                        bail!("Invalid integer: {:?}", encoded_value)
                    };
                if (integer_str.starts_with('0') && integer_str.len() > 1)
                    || integer_str.starts_with("-0")
                {
                    bail!("Invalid integer: {}", integer_str);
                }
                if let Ok(integer) = integer_str.parse::<i64>() {
                    Ok((Ben::Number(integer), 2 + integer_str.len()))
                } else {
                    bail!("Invalid integer: {:?}", encoded_value)
                }
            }
            b'l' => {
                let mut list = Vec::new();
                let mut len = 1;
                while !encoded_value[len..].is_empty() && !encoded_value[len..].starts_with(b"e") {
                    let (obj, consumed) = Self::decode_partial(&encoded_value[len..])?;
                    list.push(obj);
                    len += consumed;
                }
                if !encoded_value[len..].starts_with(b"e") {
                    bail!("Invalid list")
                }
                Ok((Ben::Array(list), len + 1))
            }
            b'd' => {
                let mut d = BTreeMap::new();
                let mut len = 1;
                while !encoded_value[len..].is_empty() && !encoded_value[len..].starts_with(b"e") {
                    let (key, consumed) = Self::decode_partial(&encoded_value[len..])?;
                    len += consumed;
                    let (value, consumed) = Self::decode_partial(&encoded_value[len..])?;
                    len += consumed;

                    d.insert(key, value);
                }
                Ok((Ben::Dict(d), len + 1))
            }
            _ => {
                panic!("Unhandled encoded value: {:?}", hex::encode(encoded_value))
            }
        }
    }
}

#[test]
fn bencoded_string() -> Result<()> {
    assert_eq!(Ben::decode(b"0:")?.to_string(), "\"\"");
    assert_eq!(Ben::decode(b"5:hello")?.to_string(), "\"hello\"");
    assert_eq!(Ben::decode_partial(b"4:hello")?, (Ben::mk_str("hell"), "4:hell".len()));
    assert!(Ben::decode(b"5:hell").is_err());
    Ok(())
}

#[test]
fn bencoded_integer() -> Result<()> {
    assert_eq!(Ben::decode(b"i0e")?.to_string(), "0");
    assert_eq!(Ben::decode(b"i10e")?.to_string(), "10");
    assert_eq!(Ben::decode(b"i-10e")?.to_string(), "-10");
    assert_eq!(
        Ben::decode(b"i9223372036854775807e")?.to_string(),
        i64::MAX.to_string()
    );
    assert_eq!(
        Ben::decode(b"i-9223372036854775808e")?.to_string(),
        i64::MIN.to_string()
    );

    assert!(Ben::decode(b"i9223372036854775808e").is_err());
    assert!(Ben::decode(b"i-9223372036854775809").is_err());

    // https://wiki.theory.org/BitTorrentSpecification#Bencoding
    assert!(Ben::decode(b"i01e").is_err());
    assert!(Ben::decode(b"i-0e").is_err());

    assert!(Ben::decode(b"i").is_err());

    Ok(())
}

#[test]
fn bencoded_list() -> Result<()> {
    assert_eq!(Ben::decode(b"le")?.to_string(), "[]");
    assert_eq!(
        Ben::decode(b"li10ei-20e2:oke")?.to_string(),
        "[10,-20,\"ok\"]"
    );
    assert_eq!(Ben::decode(b"llli1eeee")?.to_string(), "[[[1]]]");
    assert!(Ben::decode(b"llli1eee").is_err());
    Ok(())
}

#[test]
fn bencoded_dict() -> Result<()> {
    assert_eq!(
        Ben::decode(b"d3:cow3:moo4:spam4:eggse")?.to_string(),
        r#"{"cow":"moo","spam":"eggs"}"#
    );
    assert_eq!(
        Ben::decode(b"d3:cowd3:cow3:cowee")?.to_string(),
        r#"{"cow":{"cow":"cow"}}"#
    );
    assert_eq!(
        Ben::decode(b"d4:spaml1:a1:bee")?.to_string(),
        r#"{"spam":["a","b"]}"#
    );
    assert_eq!(
        Ben::decode(
            b"d9:publisher3:bob17:publisher-webpage15:www.example.com18:publisher.location4:homee"
        )?
        .to_string(),
        r#"{"publisher":"bob","publisher-webpage":"www.example.com","publisher.location":"home"}"#
    );
    Ok(())
}

#[test]
fn encode_decode() -> Result<()> {
    let torrent = include_bytes!("../sample.torrent");
    let decoded = Ben::decode(torrent)?;
    assert_eq!(&torrent[..], &decoded.encode());
    Ok(())
}
