use serde::{Deserialize, Serialize};
use std::fmt;

pub const PRESET_LATEST: u32 = 0x00FF_FFFF;
pub const FG_PRESET_DEFAULT: u32 = 0x00FF_FFFE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DlssFeature {
    Sr,
    Rr,
    Fg,
    Nr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetValueError {
    pub feature: DlssFeature,
    pub raw: u32,
}

impl fmt::Display for PresetValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "0x{:08X} is not a documented {:?} preset value",
            self.raw, self.feature
        )
    }
}

impl std::error::Error for PresetValueError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SrPreset {
    Off,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    Latest,
}

impl SrPreset {
    pub const fn raw_value(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::A => 1,
            Self::B => 2,
            Self::C => 3,
            Self::D => 4,
            Self::E => 5,
            Self::F => 6,
            Self::G => 7,
            Self::H => 8,
            Self::I => 9,
            Self::J => 10,
            Self::K => 11,
            Self::L => 12,
            Self::M => 13,
            Self::N => 14,
            Self::O => 15,
            Self::Latest => PRESET_LATEST,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Off => "SR override is off.",
            Self::A | Self::B | Self::C | Self::D => {
                "The current SR SDK marks this preset removed; the DRS numeric value remains documented."
            }
            Self::E | Self::F => {
                "The current SR SDK marks this preset deprecated; DRS runtime effectiveness is unverified."
            }
            Self::G | Self::H | Self::I | Self::N | Self::O => {
                "The current SR SDK falls back to its default for this value."
            }
            Self::J => {
                "The current SR SDK defines J as a distinct choice; DRS runtime effectiveness is unverified."
            }
            Self::K => {
                "The current SR SDK uses K as the transformer default for DLAA, Balanced, and Quality."
            }
            Self::L => {
                "The current SR SDK uses L as the transformer default for Ultra Performance."
            }
            Self::M => {
                "The current SR SDK uses M as the transformer default for Performance."
            }
            Self::Latest => {
                "The SR DRS namespace defines Latest; effective model selection is unverified."
            }
        }
    }
}

impl TryFrom<u32> for SrPreset {
    type Error = PresetValueError;

    fn try_from(raw: u32) -> Result<Self, Self::Error> {
        match raw {
            0 => Ok(Self::Off),
            1 => Ok(Self::A),
            2 => Ok(Self::B),
            3 => Ok(Self::C),
            4 => Ok(Self::D),
            5 => Ok(Self::E),
            6 => Ok(Self::F),
            7 => Ok(Self::G),
            8 => Ok(Self::H),
            9 => Ok(Self::I),
            10 => Ok(Self::J),
            11 => Ok(Self::K),
            12 => Ok(Self::L),
            13 => Ok(Self::M),
            14 => Ok(Self::N),
            15 => Ok(Self::O),
            PRESET_LATEST => Ok(Self::Latest),
            _ => Err(PresetValueError {
                feature: DlssFeature::Sr,
                raw,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RrPreset {
    Off,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    Latest,
}

impl RrPreset {
    pub const fn raw_value(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::A => 1,
            Self::B => 2,
            Self::C => 3,
            Self::D => 4,
            Self::E => 5,
            Self::F => 6,
            Self::G => 7,
            Self::H => 8,
            Self::I => 9,
            Self::J => 10,
            Self::K => 11,
            Self::L => 12,
            Self::M => 13,
            Self::N => 14,
            Self::O => 15,
            Self::Latest => PRESET_LATEST,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Off => "RR override is off.",
            Self::A | Self::B | Self::C => {
                "The current RR SDK marks this preset removed; the DRS numeric value remains documented."
            }
            Self::D => {
                "The current RR SDK defines D as a transformer preset; DRS runtime effectiveness is unverified."
            }
            Self::E => {
                "The current RR SDK defines E as a transformer preset; DRS runtime effectiveness is unverified."
            }
            Self::F => "The current RR SDK defines F as the latest/default transformer.",
            Self::G
            | Self::H
            | Self::I
            | Self::J
            | Self::K
            | Self::L
            | Self::M
            | Self::N
            | Self::O => "The current RR SDK falls back to its default for this value.",
            Self::Latest => {
                "The RR DRS namespace defines Latest; effective model selection is unverified."
            }
        }
    }
}

impl TryFrom<u32> for RrPreset {
    type Error = PresetValueError;

    fn try_from(raw: u32) -> Result<Self, Self::Error> {
        match raw {
            0 => Ok(Self::Off),
            1 => Ok(Self::A),
            2 => Ok(Self::B),
            3 => Ok(Self::C),
            4 => Ok(Self::D),
            5 => Ok(Self::E),
            6 => Ok(Self::F),
            7 => Ok(Self::G),
            8 => Ok(Self::H),
            9 => Ok(Self::I),
            10 => Ok(Self::J),
            11 => Ok(Self::K),
            12 => Ok(Self::L),
            13 => Ok(Self::M),
            14 => Ok(Self::N),
            15 => Ok(Self::O),
            PRESET_LATEST => Ok(Self::Latest),
            _ => Err(PresetValueError {
                feature: DlssFeature::Rr,
                raw,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FgPreset {
    Off,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Default,
    Latest,
}

impl FgPreset {
    pub const fn raw_value(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::A => 1,
            Self::B => 2,
            Self::C => 3,
            Self::D => 4,
            Self::E => 5,
            Self::F => 6,
            Self::G => 7,
            Self::H => 8,
            Self::I => 9,
            Self::J => 10,
            Self::K => 11,
            Self::L => 12,
            Self::M => 13,
            Self::N => 14,
            Self::O => 15,
            Self::P => 16,
            Self::Q => 17,
            Self::R => 18,
            Self::S => 19,
            Self::T => 20,
            Self::U => 21,
            Self::V => 22,
            Self::W => 23,
            Self::X => 24,
            Self::Y => 25,
            Self::Z => 26,
            Self::Default => FG_PRESET_DEFAULT,
            Self::Latest => PRESET_LATEST,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Off => "FG override is off.",
            Self::Default => "The FG DRS namespace defines Default.",
            Self::Latest => {
                "The FG DRS namespace defines Latest separately from Default."
            }
            _ => {
                "The FG DRS namespace defines this letter numerically; effective model behavior is unverified."
            }
        }
    }
}

impl TryFrom<u32> for FgPreset {
    type Error = PresetValueError;

    fn try_from(raw: u32) -> Result<Self, Self::Error> {
        match raw {
            0 => Ok(Self::Off),
            1 => Ok(Self::A),
            2 => Ok(Self::B),
            3 => Ok(Self::C),
            4 => Ok(Self::D),
            5 => Ok(Self::E),
            6 => Ok(Self::F),
            7 => Ok(Self::G),
            8 => Ok(Self::H),
            9 => Ok(Self::I),
            10 => Ok(Self::J),
            11 => Ok(Self::K),
            12 => Ok(Self::L),
            13 => Ok(Self::M),
            14 => Ok(Self::N),
            15 => Ok(Self::O),
            16 => Ok(Self::P),
            17 => Ok(Self::Q),
            18 => Ok(Self::R),
            19 => Ok(Self::S),
            20 => Ok(Self::T),
            21 => Ok(Self::U),
            22 => Ok(Self::V),
            23 => Ok(Self::W),
            24 => Ok(Self::X),
            25 => Ok(Self::Y),
            26 => Ok(Self::Z),
            FG_PRESET_DEFAULT => Ok(Self::Default),
            PRESET_LATEST => Ok(Self::Latest),
            _ => Err(PresetValueError {
                feature: DlssFeature::Fg,
                raw,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NrPreset {
    Off,
    A,
    B,
    C,
    D,
    Latest,
}

impl NrPreset {
    pub const fn raw_value(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::A => 1,
            Self::B => 2,
            Self::C => 3,
            Self::D => 4,
            Self::Latest => PRESET_LATEST,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Off => "NR override is off.",
            Self::Latest => {
                "The NR DRS namespace defines Latest; applicability remains unverified."
            }
            _ => {
                "The NR DRS namespace defines this letter numerically; model meaning and applicability remain unverified."
            }
        }
    }
}

impl TryFrom<u32> for NrPreset {
    type Error = PresetValueError;

    fn try_from(raw: u32) -> Result<Self, Self::Error> {
        match raw {
            0 => Ok(Self::Off),
            1 => Ok(Self::A),
            2 => Ok(Self::B),
            3 => Ok(Self::C),
            4 => Ok(Self::D),
            PRESET_LATEST => Ok(Self::Latest),
            _ => Err(PresetValueError {
                feature: DlssFeature::Nr,
                raw,
            }),
        }
    }
}
