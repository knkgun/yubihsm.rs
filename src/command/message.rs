//! Messages sent to/from the `YubiHSM2`, i.e Application Protocol Data Units
//! (a.k.a. APDU)
//!
//! Documentation for the available command and their message structure
//! is available on Yubico's `YubiHSM2` web site:
//!
//! <https://developers.yubico.com/YubiHSM2/Commands/>

// TODO: this code predates the serde serializers. It could be rewritten with serde.

#[cfg(feature = "mockhsm")]
use byteorder::ByteOrder;
use byteorder::{BigEndian, WriteBytesExt};
use uuid::Uuid;

use super::{CommandCode, MAX_MSG_SIZE};
use crate::session::{
    securechannel::{Mac, MAC_SIZE},
    SessionError,
    SessionErrorKind::ProtocolError,
    SessionId,
};

/// A command sent from the host to the `YubiHSM2`. May or may not be
/// authenticated using SCP03's chained/evolving MAC protocol.
#[derive(Debug)]
pub(crate) struct CommandMessage {
    /// UUID which uniquely identifies this command
    pub uuid: Uuid,

    /// Type of command to be invoked
    pub command_type: CommandCode,

    /// Session ID for this command
    pub session_id: Option<SessionId>,

    /// Command Data field (i.e. message payload)
    pub data: Vec<u8>,

    /// Optional Message Authentication Code (MAC)
    pub mac: Option<Mac>,
}

impl CommandMessage {
    /// Create a new command message without a MAC
    pub fn create<T>(command_type: CommandCode, command_data: T) -> Result<Self, SessionError>
    where
        T: Into<Vec<u8>>,
    {
        let command_data_vec: Vec<u8> = command_data.into();

        ensure!(
            command_data_vec.len() <= MAX_MSG_SIZE,
            ProtocolError,
            "command data too long: {} bytes (max {})",
            command_data_vec.len(),
            MAX_MSG_SIZE
        );

        Ok(Self {
            uuid: Uuid::new_v4(),
            command_type,
            session_id: None,
            data: command_data_vec,
            mac: None,
        })
    }

    /// Create a new command message with a MAC
    pub fn new_with_mac<D, M>(
        command_type: CommandCode,
        session_id: SessionId,
        command_data: D,
        mac: M,
    ) -> Result<Self, SessionError>
    where
        D: Into<Vec<u8>>,
        M: Into<Mac>,
    {
        let command_data_vec: Vec<u8> = command_data.into();

        ensure!(
            command_data_vec.len() <= MAX_MSG_SIZE,
            ProtocolError,
            "command data too long: {} bytes (max {})",
            command_data_vec.len(),
            MAX_MSG_SIZE
        );

        Ok(Self {
            uuid: Uuid::new_v4(),
            command_type,
            session_id: Some(session_id),
            data: command_data_vec,
            mac: Some(mac.into()),
        })
    }

    /// Parse a command structure from a vector, taking ownership of the vector
    #[cfg(feature = "mockhsm")]
    pub fn parse(mut bytes: Vec<u8>) -> Result<Self, SessionError> {
        if bytes.len() < 3 {
            fail!(
                ProtocolError,
                "command too short: {} (expected at least 3-bytes)",
                bytes.len()
            );
        }

        let command_type =
            CommandCode::from_u8(bytes[0]).map_err(|e| err!(ProtocolError, "{}", e))?;

        let length = BigEndian::read_u16(&bytes[1..3]) as usize;

        if length + 3 != bytes.len() {
            fail!(
                ProtocolError,
                "unexpected command length {} (expecting {})",
                bytes.len() - 3,
                length
            );
        }

        bytes.drain(..3);

        let (session_id, mac) = match command_type {
            CommandCode::AuthenticateSession | CommandCode::SessionMessage => {
                if bytes.is_empty() {
                    fail!(
                        ProtocolError,
                        "expected session ID but command data is empty"
                    );
                }

                let id = SessionId::from_u8(bytes.remove(0))?;

                if bytes.len() < MAC_SIZE {
                    fail!(
                        ProtocolError,
                        "expected MAC for {:?} but command data is too short: {}",
                        command_type,
                        bytes.len(),
                    );
                }

                let mac_index = bytes.len() - MAC_SIZE;

                (Some(id), Some(Mac::from_slice(&bytes.split_off(mac_index))))
            }
            _ => (None, None),
        };

        Ok(Self {
            uuid: Uuid::new_v4(),
            command_type,
            session_id,
            data: bytes,
            mac,
        })
    }

    /// Calculate the length of the serialized message, sans command type and length field
    pub fn len(&self) -> usize {
        let mut result = self.data.len();

        if self.session_id.is_some() {
            result += 1;
        }

        if self.mac.is_some() {
            result += MAC_SIZE;
        }

        result
    }
}

impl Into<Vec<u8>> for CommandMessage {
    /// Serialize this Command, consuming it and creating a Vec<u8>
    fn into(mut self) -> Vec<u8> {
        let mut result = Vec::with_capacity(3 + self.len());
        result.push(self.command_type as u8);
        result.write_u16::<BigEndian>(self.len() as u16).unwrap();

        if let Some(session_id) = self.session_id {
            result.push(session_id.to_u8());
        }

        result.append(&mut self.data);

        if let Some(mac) = self.mac {
            result.extend_from_slice(mac.as_slice());
        }

        result
    }
}
