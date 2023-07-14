use std::{io::Cursor, marker::PhantomData};

use bytes::{Buf, BytesMut};
use color_eyre::Report;
use thiserror::Error;
use tokio::{io::AsyncReadExt, net::tcp::OwnedReadHalf};

use crate::data::GeneralError;

pub struct FrameReader<T> {
    inner: OwnedReadHalf,
    buffer: BytesMut,
    item: PhantomData<T>,
}

impl<T> FrameReader<T>
where
    T: ParseCheck,
{
    pub fn new(inner: OwnedReadHalf) -> Self {
        Self {
            inner,
            buffer: BytesMut::with_capacity(1024),
            item: PhantomData,
        }
    }

    pub async fn read_frame(&mut self) -> Result<Option<T>, Report> {
        loop {
            if let Some(res) = self.parse_frame()? {
                return Ok(Some(res));
            }

            if 0 == self.inner.read_buf(&mut self.buffer).await? {
                if self.buffer.is_empty() {
                    return Ok(None);
                } else {
                    return Err(GeneralError::BrokenPipe.into());
                }
            }
        }
    }

    fn parse_frame(&mut self) -> Result<Option<T>, Report> {
        let mut buf = Cursor::new(&self.buffer[..]);

        match T::check(&mut buf) {
            Ok(_) => {
                let len = buf.position() as usize;
                buf.set_position(0);
                let frame = T::parse(&mut buf)?;

                self.buffer.advance(len);

                Ok(Some(frame))
            }
            Err(ParseError::Incomplete) => Ok(None),
        }
    }
}

pub trait ParseCheck {
    fn get_u8(v: &mut Cursor<&[u8]>) -> Option<u8> {
        if !v.has_remaining() {
            return None;
        }

        Some(v.get_u8())
    }

    fn check(v: &mut Cursor<&[u8]>) -> Result<(), ParseError>;
    fn parse(v: &mut Cursor<&[u8]>) -> Result<Self, ParseError>
    where
        Self: Sized;
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("incomplete")]
    Incomplete,
}
