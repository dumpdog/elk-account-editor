//! Port of the offline account-file units of Elk.Protocol: `AccountFile`, `AccountModel`, `AccountExport`, `ZoneImport`,
//! `ZoneSheet` and `AlarmZoneList`. Behaviour (bytes, JSON, spreadsheet cells) matches the C# reference.

pub mod account_export;
pub mod account_file;
pub mod account_model;
pub mod alarm_zone_list;
pub mod automation;
pub mod details;
pub mod english;
pub mod error;
pub mod json;
pub mod records;
pub mod rules;
pub mod xml;
pub mod zone_import;
pub mod zone_sheet;

pub use account_export::{AccountExport, ZoneBlock};
pub use account_file::{decode_latin1, encode_latin1, split_csv_line, AccountFile, Entry, Section};
pub use account_model::{
    AccountModel, AreaDto, AreaRcDto, CommDto, FKeyDto, FlagDef, KeypadDto, KeypadRcDto, ModelMeta,
    PhoneDto, SystemRcDto, UserDto, UserRcDto, ZoneDto, ZoneRcDto,
};
pub use alarm_zone_list::AlarmZoneList;
pub use automation::{
    AutomationBlock, AutomationCodec, AutomationDto, AutomationMeta, CustomDto, LightDto,
    NamedItemDto, OrderedMap, TextDto, VoiceMsgDto,
};
pub use details::DetailsDto;
pub use error::{Error, Result};
pub use rules::RuleDto;
pub use zone_import::{ZoneCsv, ZoneSpec, ZoneVoice};
pub use zone_sheet::{ZoneSheet, ZoneSheetResult, ZoneSheetRow};
