//! `attributes.xml`: graphics, display, audio and gameplay settings.
//!
//! The file mixes things you would want in a profile (graphics quality,
//! resolution, FOV) with things you would not want overwritten (the ship
//! selected in Arena Commander, the microphone). So settings are sorted into
//! groups by name, and a restore only replaces the groups asked for.

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Group {
    Graphics,
    Display,
    Audio,
    HeadTracking,
    Gameplay,
    Other,
}

impl Group {
    pub const ALL: [Group; 6] =
        [Group::Graphics, Group::Display, Group::Audio, Group::HeadTracking, Group::Gameplay, Group::Other];

    /// What a restore replaces when not told otherwise.
    pub const DEFAULT: [Group; 2] = [Group::Graphics, Group::Display];

    pub fn label(self) -> &'static str {
        match self {
            Group::Graphics => "Graphics",
            Group::Display => "Display & FOV",
            Group::Audio => "Audio",
            Group::HeadTracking => "Head tracking & VR",
            Group::Gameplay => "Flight, HUD & gameplay",
            Group::Other => "Other",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Group::Graphics => "Quality levels, upscaling, motion blur, film grain, HDR",
            Group::Display => "Resolution, window mode, field of view",
            Group::Audio => "Volumes and the ship computer voice",
            Group::HeadTracking => "TrackIR, Tobii, webcam tracking and headset settings",
            Group::Gameplay => "Look-ahead, flight HUD, crosshair, sensitivity and the rest",
            Group::Other => "Selected ships, region, microphone and anything unrecognised",
        }
    }

    /// The name used on the command line.
    pub fn cli_name(self) -> &'static str {
        match self {
            Group::Graphics => "graphics",
            Group::Display => "display",
            Group::Audio => "audio",
            Group::HeadTracking => "head-tracking",
            Group::Gameplay => "gameplay",
            Group::Other => "other",
        }
    }

    pub fn from_cli_name(s: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.cli_name() == s)
    }
}

/// Which group a setting belongs to.
pub fn group_of(name: &str) -> Group {
    let lower = name.to_ascii_lowercase();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| lower.starts_with(p));
    if starts(&["sysspec", "upscaling"])
        || matches!(
            lower.as_str(),
            "chromaticaberration" | "filmgrain" | "motionblur" | "gamma" | "autodetect" | "vsync"
        )
        || lower.starts_with("hdr")
    {
        Group::Graphics
    } else if matches!(
        lower.as_str(),
        "width" | "height" | "resolution" | "windowmode" | "fov" | "aspectmodifier"
    ) || lower.starts_with("overscan")
    {
        Group::Display
    } else if lower.starts_with("audio") || lower == "videovolume" {
        Group::Audio
    } else if starts(&["headtracking", "tobii", "hmd", "trackir", "facetracking"]) {
        Group::HeadTracking
    } else if starts(&["selectedship", "preferred", "preset", "qrcode"]) {
        Group::Other
    } else {
        Group::Gameplay
    }
}

/// The settings in file order.
pub fn read(xml: &str) -> Result<Vec<(String, String)>> {
    let doc = roxmltree::Document::parse(xml).context("attributes.xml is not valid XML")?;
    Ok(doc
        .descendants()
        .filter(|n| n.has_tag_name("Attr"))
        .filter_map(|n| Some((n.attribute("name")?.to_string(), n.attribute("value")?.to_string())))
        .collect())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergeStats {
    pub changed: usize,
    pub added: usize,
}

/// Copy the settings in `groups` from `saved` into `live`. Settings in other
/// groups, and anything `saved` does not mention, are left alone. Formatting
/// of untouched lines is kept exactly.
pub fn merge(live: &str, saved: &str, groups: &BTreeSet<Group>) -> Result<(String, MergeStats)> {
    let wanted: BTreeMap<String, String> =
        read(saved)?.into_iter().filter(|(name, _)| groups.contains(&group_of(name))).collect();
    let mut remaining = wanted.clone();
    let newline = if live.contains("\r\n") { "\r\n" } else { "\n" };

    let mut reader = Reader::from_str(live);
    let mut writer = Writer::new(Vec::with_capacity(live.len()));
    let mut stats = MergeStats::default();
    loop {
        match reader.read_event().context("attributes.xml is not valid XML")? {
            Event::Eof => break,
            Event::Empty(tag) if tag.local_name().as_ref() == "Attr" => {
                let tag = set_value(tag, &wanted, &mut remaining, &mut stats)?;
                writer.write_event(Event::Empty(tag))?;
            }
            Event::End(end) if end.local_name().as_ref() == "Attributes" => {
                for (name, value) in std::mem::take(&mut remaining) {
                    writer.get_mut().extend_from_slice(
                        format!(" <Attr name=\"{}\" value=\"{}\"/>{newline}", esc(&name), esc(&value))
                            .as_bytes(),
                    );
                    stats.added += 1;
                }
                writer.write_event(Event::End(end))?;
            }
            other => writer.write_event(other)?,
        }
    }
    Ok((String::from_utf8(writer.into_inner())?, stats))
}

fn set_value<'a>(
    tag: BytesStart<'a>,
    wanted: &BTreeMap<String, String>,
    remaining: &mut BTreeMap<String, String>,
    stats: &mut MergeStats,
) -> Result<BytesStart<'a>> {
    let mut name = None;
    let mut raw_value = None;
    for attr in tag.attributes() {
        let attr = attr?;
        let raw = attr.value.to_string();
        let value = quick_xml::escape::unescape(&raw).map(|v| v.into_owned()).unwrap_or_else(|_| raw.clone());
        match attr.key.as_ref() {
            "name" => name = Some(value),
            "value" => raw_value = Some((raw, value)),
            _ => {}
        }
    }
    let (Some(name), Some((raw, current))) = (name, raw_value) else {
        return Ok(tag);
    };
    let Some(new) = wanted.get(&name) else {
        return Ok(tag);
    };
    remaining.remove(&name);
    if *new == current {
        return Ok(tag);
    }
    let old = format!("value=\"{raw}\"");
    let text: String = (*tag).to_string();
    let Some(pos) = text.find(&old) else {
        return Ok(tag);
    };
    let mut text = text;
    text.replace_range(pos..pos + old.len(), &format!("value=\"{}\"", esc(new)));
    stats.changed += 1;
    let name_len = tag.name().as_ref().len();
    Ok(BytesStart::from_content(text, name_len))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        assert_eq!(group_of("SysSpec_ShadowMaps"), Group::Graphics);
        assert_eq!(group_of("MotionBlur"), Group::Graphics);
        assert_eq!(group_of("HDRMaxBrightness"), Group::Graphics);
        assert_eq!(group_of("FOV"), Group::Display);
        assert_eq!(group_of("WindowMode"), Group::Display);
        assert_eq!(group_of("AudioMusicVolume"), Group::Audio);
        assert_eq!(group_of("HeadtrackingSource"), Group::HeadTracking);
        assert_eq!(group_of("HeadTrackingFaceWareDeadzoneRotationRoll"), Group::HeadTracking);
        assert_eq!(group_of("TobiiHeadPositionScale"), Group::HeadTracking);
        assert_eq!(group_of("LookAheadStrengthRoll"), Group::Gameplay);
        assert_eq!(group_of("selectedShipURN_EA_Duel"), Group::Other);
        assert_eq!(group_of("PreferredAudioInputDeviceHash"), Group::Other);
    }

    #[test]
    fn cli_names_round_trip() {
        for g in Group::ALL {
            assert_eq!(Group::from_cli_name(g.cli_name()), Some(g));
        }
    }
}
