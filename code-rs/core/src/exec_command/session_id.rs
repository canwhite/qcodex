use serde::de::Error as _;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub(crate) struct SessionId(pub u32);

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = code_protocol::models::deserialize_integer_valued_u64(deserializer)?;
        u32::try_from(value)
            .map(Self)
            .map_err(D::Error::custom)
    }
}
