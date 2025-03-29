use crate::byte_view::ByteView;
use anyhow::Result;
use bitcoin::consensus::{
    deserialize_partial,
    encode::{Decodable, Encodable},
};
use std::io::BufRead;
use std::io::Read;
use std::mem::size_of;

pub fn consensus_encode<T: Encodable>(v: &T) -> Result<Vec<u8>> {
    let mut result = Vec::<u8>::new();
    <T as Encodable>::consensus_encode::<Vec<u8>>(v, &mut result)?;
    Ok(result)
}

pub fn consensus_decode<T: Decodable>(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<T> {
    let slice = &cursor.get_ref()[cursor.position() as usize..cursor.get_ref().len() as usize];
    let deserialized: (T, usize) = deserialize_partial(slice)?;
    cursor.consume(deserialized.1);
    Ok(deserialized.0)
}

pub fn consume_sized_int<T: ByteView>(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<T> {
    let buffer = consume_exact(cursor, size_of::<T>())?;
    Ok(T::from_bytes(buffer))
}

pub fn consume_to_end(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<Vec<u8>> {
    if is_empty(cursor) {
        return Ok(vec![]);
    }
    let mut result: Vec<u8> = vec![];
    cursor.read_to_end(&mut result)?;
    Ok(result)
}

pub fn consume_exact(cursor: &mut std::io::Cursor<Vec<u8>>, n: usize) -> Result<Vec<u8>> {
    let mut buffer: Vec<u8> = vec![0u8; n];
    cursor.read_exact(&mut buffer[0..n])?;
    Ok(buffer)
}

pub fn consume_varint(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<u64> {
    Ok(match consume_sized_int::<u8>(cursor)? {
        0xff => consume_sized_int::<u64>(cursor)?,
        0xfe => consume_sized_int::<u32>(cursor)? as u64,
        0xfd => consume_sized_int::<u16>(cursor)? as u64,
        v => v as u64,
    })
}

pub fn consume_u128(cursor: &mut std::io::Cursor<Vec<u8>>) -> Result<u128> {
    consume_sized_int::<u128>(cursor)
}

pub fn is_empty(cursor: &mut std::io::Cursor<Vec<u8>>) -> bool {
    cursor.position() >= cursor.get_ref().len() as u64
}

pub fn remaining_slice(cursor: &mut std::io::Cursor<Vec<u8>>) -> &[u8] {
    &cursor.get_ref()[(cursor.position() as usize)..cursor.get_ref().len()]
}
pub fn ptr_to_vec(ptr: i32) -> Vec<u8> {
    unsafe {
        let len = *((ptr - 4) as usize as *const usize);
        Box::leak(Box::new(Vec::<u8>::from_raw_parts(
            ptr as usize as *mut u8,
            len,
            len,
        )))
        .clone()
    }
}

pub fn format_key(v: &Vec<u8>) -> String {
    v.clone()
        .split(|c| *c == 47)
        .map(|bytes| {
            let v = bytes.to_vec();
            if v.len() == 0 {
                return "".to_owned();
            }
            let r = String::from_utf8(v);
            let is_ascii = match r {
                Ok(ref s) => s.is_ascii(),
                Err(_) => false,
            };
            if is_ascii {
                "/".to_owned() + r.unwrap().as_str()
            } else {
                "/".to_owned() + hex::encode(bytes).as_str()
            }
        })
        .collect::<Vec<String>>()
        .join("")
}
