// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Codec to parse the bits that are the same for every bundle2, except for
//! stream-level parameters (see `stream_start` for those). This parses bundle2
//! part headers and puts together chunks for inner codecs to parse.

use std::io::BufRead;
use std::mem;

use async_compression::Decompressor;
use bytes::{Bytes, BytesMut};
use futures_ext::io::Either::{self, A as UncompressedRead, B as CompressedRead};
use slog;
use tokio_io::AsyncRead;
use tokio_io::codec::{Decoder, Framed, FramedParts};

use errors::*;
use part_header::{self, PartHeader, PartHeaderType};
use part_inner::validate_header;
use types::StreamHeader;
use utils::{get_decompressor_type, BytesExt};

pub fn outer_stream<'a, R: AsyncRead + BufRead + Send>(
    stream_header: &StreamHeader,
    r: R,
    logger: &slog::Logger,
) -> Result<OuterStream<'a, R>> {
    let decompressor_type = get_decompressor_type(
        stream_header
            .m_stream_params
            .get("compression")
            .map(String::as_ref),
    )?;

    Ok(Framed::from_parts(
        FramedParts {
            inner: match decompressor_type {
                None => UncompressedRead(r),
                Some(decompressor_type) => CompressedRead(Decompressor::new(r, decompressor_type)),
            },
            readbuf: BytesMut::new(),
            writebuf: BytesMut::new(),
        },
        OuterDecoder::new(logger.new(o!("stream" => "outer"))),
    ))
}

pub type OuterStream<'a, R> = Framed<Either<R, Decompressor<'a, R>>, OuterDecoder>;

#[derive(Debug)]
enum OuterState {
    Header,
    Payload {
        part_type: PartHeaderType,
        part_id: u32,
    },
    DiscardPayload,
    StreamEnd,
    Invalid,
}

impl OuterState {
    pub fn take(&mut self) -> Self {
        mem::replace(self, OuterState::Invalid)
    }

    pub fn payload_frame(&self, data: BytesMut) -> OuterFrame {
        match self {
            &OuterState::Payload {
                ref part_type,
                ref part_id,
            } => OuterFrame::Payload {
                part_type: part_type.clone(),
                part_id: *part_id,
                payload: data.freeze(),
            },
            &OuterState::DiscardPayload => OuterFrame::Discard,
            _ => panic!("payload_frame called for state without payloads"),
        }
    }

    pub fn part_end_frame(self) -> OuterFrame {
        match self {
            OuterState::Payload { part_type, part_id } => OuterFrame::PartEnd {
                part_type: part_type,
                part_id: part_id,
            },
            OuterState::DiscardPayload => OuterFrame::Discard,
            _ => panic!("part_end_frame called for state without payloads"),
        }
    }
}

#[derive(Debug)]
pub struct OuterDecoder {
    logger: slog::Logger,
    state: OuterState,
}

impl Decoder for OuterDecoder {
    type Item = OuterFrame;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        let (ret, next_state) = Self::decode_next(buf, self.state.take(), &self.logger);
        self.state = next_state;
        ret
    }
}

impl OuterDecoder {
    pub fn new(logger: slog::Logger) -> Self {
        OuterDecoder {
            logger: logger,
            state: OuterState::Header,
        }
    }

    fn decode_next(
        buf: &mut BytesMut,
        mut state: OuterState,
        _logger: &slog::Logger,
    ) -> (Result<Option<OuterFrame>>, OuterState) {
        // TODO: the only state valid when the stream terminates is
        // StreamEnd. Communicate that to callers.
        match state.take() {
            OuterState::Header => {
                // The header is structured as:
                // ---
                // header_len: u32
                // header: header_len bytes
                // ---
                // See part_header::decode for information about the internal structure.
                if buf.len() < 4 {
                    return (Ok(None), OuterState::Header);
                }

                let header_len = buf.peek_u32() as usize;
                if buf.len() < 4 + header_len {
                    return (Ok(None), OuterState::Header);
                }

                let _ = buf.split_to(4);
                if header_len == 0 {
                    // A zero-length header indicates that the stream has ended.
                    return (Ok(Some(OuterFrame::StreamEnd)), OuterState::StreamEnd);
                }

                let part_header = Self::decode_header(buf.split_to(header_len).freeze());
                if let Err(e) = part_header {
                    let next = match e.downcast::<ErrorKind>() {
                        Ok(ek) => if ek.is_app_error() {
                            (Err(ek.into()), OuterState::DiscardPayload)
                        } else {
                            (Err(ek.into()), OuterState::Invalid)
                        },
                        Err(e) => (Err(e), OuterState::Invalid),
                    };
                    return next;
                };
                let part_header = part_header.unwrap();
                // If no part header was returned, this part wasn't
                // recognized. Throw it away.
                match part_header {
                    None => (Ok(Some(OuterFrame::Discard)), OuterState::DiscardPayload),
                    Some(header) => {
                        let part_type = *header.part_type();
                        let part_id = header.part_id();
                        (
                            Ok(Some(OuterFrame::Header(header))),
                            OuterState::Payload {
                                part_type: part_type,
                                part_id: part_id,
                            },
                        )
                    }
                }
            }

            cur_state @ OuterState::Payload { .. } | cur_state @ OuterState::DiscardPayload => {
                let (payload, next_state) = Self::decode_payload(buf, cur_state);
                (payload.map_err(|e| e.into()), next_state)
            }

            OuterState::StreamEnd => (Ok(Some(OuterFrame::StreamEnd)), OuterState::StreamEnd),

            OuterState::Invalid => (
                Err(ErrorKind::Bundle2Decode("byte stream corrupt".into()).into()),
                OuterState::Invalid,
            ),
        }
    }

    fn decode_header(header_bytes: Bytes) -> Result<Option<PartHeader>> {
        let header = part_header::decode(header_bytes)?;
        match validate_header(header)? {
            Some(header) => Ok(Some(header)),
            None => {
                // The part couldn't be recognized but wasn't important anyway.
                // Throw it away (the state machine will throw away any associated
                // chunks it finds).
                Ok(None)
            }
        }
    }

    fn decode_payload(
        buf: &mut BytesMut,
        state: OuterState,
    ) -> (Result<Option<OuterFrame>>, OuterState) {
        if buf.len() < 4 {
            return (Ok(None), state);
        }

        // Payloads are in the format:
        // ---
        // total_len: i32
        // payload: Vec<u8>, total_len bytes
        // ---
        // A payload is guaranteed to be < 2**31 bytes, so buffer up
        // until the whole payload is available.
        //
        // TODO: -1 means this part has been interrupted. Handle that
        // case.

        let total_len = buf.peek_i32();
        if total_len == 0 {
            let _ = buf.drain_i32();
            // A zero-size chunk indicates that this part has
            // ended. More parts might be coming up, so go back to the
            // header state.
            (Ok(Some(state.part_end_frame())), OuterState::Header)
        } else {
            let payload = Self::decode_payload_chunk(buf, &state, total_len as usize);
            (Ok(payload), state)
        }
    }

    fn decode_payload_chunk(
        buf: &mut BytesMut,
        state: &OuterState,
        total_len: usize,
    ) -> Option<OuterFrame> {
        // + 4 bytes for the header
        if buf.len() < total_len + 4 {
            return None;
        }

        let _ = buf.drain_i32();
        let chunk = buf.split_to(total_len);

        Some(state.payload_frame(chunk))
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum OuterFrame {
    Header(PartHeader),
    Payload {
        part_type: PartHeaderType,
        part_id: u32,
        payload: Bytes,
    },
    PartEnd {
        part_type: PartHeaderType,
        part_id: u32,
    },
    Discard,
    StreamEnd,
}

impl OuterFrame {
    pub fn is_payload(&self) -> bool {
        match self {
            &OuterFrame::Payload { .. } => true,
            _ => false,
        }
    }

    pub fn get_payload(self) -> Bytes {
        match self {
            OuterFrame::Payload { payload, .. } => payload,
            _ => panic!("get_payload called on an OuterFrame without a payload!"),
        }
    }
}
