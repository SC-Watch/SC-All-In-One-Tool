//! Star Citizen bindings files: the live `actionmaps.xml` and layouts
//! exported from the game's keybinding screen.
//!
//! A joystick's number appears in three places:
//!
//! * `<options type="joystick" instance="3" Product="...">`, which says which
//!   device the number belongs to and holds its invert settings;
//! * every binding, as the prefix of `input="js3_button1"`. Empty ones
//!   (`input="js3_ "`) switch off one of the game's default bindings on that
//!   device, so they belong to the device too;
//! * in exported layouts, `<devices><joystick instance="3"/>`.
//!
//! Deadzones, saturation and curves live in `<deviceoptions name="...">`,
//! which is keyed by device name, so renumbering never touches it.

use crate::device::DeviceId;
use anyhow::{Context, Result, bail};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A joystick the file names, and the number it has in that file.
#[derive(Clone, Debug)]
pub struct Joystick {
    pub instance: u32,
    pub device: DeviceId,
}

/// What a bindings file says about joysticks.
#[derive(Clone, Debug, Default)]
pub struct ActionMaps {
    /// `options` entries that name a device, in file order.
    pub joysticks: Vec<Joystick>,
    /// For each js number, what its bindings are, without the number:
    /// `"spaceship_movement/v_strafe_lateral/x"`. Used to recognise a
    /// device's bindings after they have been renumbered.
    pub fingerprints: BTreeMap<u32, BTreeSet<String>>,
    /// For each js number: (bindings, cleared defaults).
    pub counts: BTreeMap<u32, (usize, usize)>,
    /// Names from `deviceoptions`: devices with saturation, deadzone or curve
    /// settings, connected or not.
    pub device_settings: Vec<String>,
}

/// Split `js3_rctrl+button21` into `(3, "rctrl+button21")`.
pub fn split_js_input(input: &str) -> Option<(u32, &str)> {
    let rest = input.strip_prefix("js")?;
    let underscore = rest.find('_')?;
    let n = rest[..underscore].parse().ok()?;
    Some((n, &rest[underscore + 1..]))
}

impl ActionMaps {
    pub fn parse(xml: &str) -> Result<Self> {
        let doc = roxmltree::Document::parse(xml).context("the bindings file is not valid XML")?;
        let mut out = ActionMaps::default();
        for node in doc.descendants().filter(|n| n.is_element()) {
            match node.tag_name().name() {
                "options" if node.attribute("type") == Some("joystick") => {
                    if let (Some(instance), Some(product)) =
                        (node.attribute("instance"), node.attribute("Product"))
                        && let Ok(instance) = instance.trim().parse()
                    {
                        out.joysticks.push(Joystick { instance, device: DeviceId::parse(product) });
                    }
                }
                "deviceoptions" => {
                    if let Some(name) = node.attribute("name") {
                        out.device_settings.push(DeviceId::parse(name).name);
                    }
                }
                _ => {}
            }
            let Some((n, suffix)) = node.attribute("input").and_then(split_js_input) else {
                continue;
            };
            let action =
                node.ancestors().find(|a| a.has_tag_name("action")).and_then(|a| a.attribute("name"));
            let map =
                node.ancestors().find(|a| a.has_tag_name("actionmap")).and_then(|a| a.attribute("name"));
            let place = match (map, action) {
                (Some(m), Some(a)) => format!("{m}/{a}"),
                _ => node.tag_name().name().to_string(),
            };
            out.fingerprints.entry(n).or_default().insert(format!("{place}/{suffix}"));
            let count = out.counts.entry(n).or_default();
            if suffix.trim().is_empty() { count.1 += 1 } else { count.0 += 1 }
        }
        Ok(out)
    }

    pub fn instance_of(&self, device: &DeviceId) -> Option<u32> {
        let key = device.key();
        self.joysticks.iter().find(|j| j.device.key() == key).map(|j| j.instance)
    }

    pub fn device_at(&self, instance: u32) -> Option<&DeviceId> {
        self.joysticks.iter().find(|j| j.instance == instance).map(|j| &j.device)
    }

    /// Bindings (not counting cleared defaults) on a js number.
    pub fn bindings_on(&self, instance: u32) -> usize {
        self.counts.get(&instance).map_or(0, |c| c.0)
    }

    /// Keys that more than one joystick in the file shares: two identical
    /// devices the game cannot tell apart.
    pub fn duplicate_keys(&self) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut dupes = BTreeSet::new();
        for j in &self.joysticks {
            if !seen.insert(j.device.key()) {
                dupes.insert(j.device.key());
            }
        }
        dupes
    }
}

/// How to renumber a bindings file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Renumber {
    /// Old js number → new js number, for bindings and layout headers.
    /// Numbers not listed stay as they are.
    pub inputs: BTreeMap<u32, u32>,
    /// Device key → the number its `options` entry should have.
    pub options: HashMap<String, u32>,
}

impl Renumber {
    pub fn is_identity(&self) -> bool {
        self.inputs.iter().all(|(a, b)| a == b)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenumberStats {
    pub inputs_changed: usize,
    pub options_changed: usize,
}

/// Apply a renumbering. Everything not renumbered is written back byte for
/// byte, including spacing, line endings and attribute order.
pub fn renumber(xml: &str, plan: &Renumber) -> Result<(String, RenumberStats)> {
    let targets: BTreeSet<u32> = plan.inputs.values().copied().collect();
    if targets.len() != plan.inputs.len() {
        bail!("internal error: two joysticks would be given the same number");
    }
    let mut reader = Reader::from_str(xml);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut stats = RenumberStats::default();
    loop {
        let event = reader.read_event().context("the bindings file is not valid XML")?;
        match event {
            Event::Eof => break,
            Event::Start(start) => {
                let start = renumber_tag(start, plan, &mut stats)?;
                writer.write_event(Event::Start(start))?;
            }
            Event::Empty(start) => {
                let start = renumber_tag(start, plan, &mut stats)?;
                writer.write_event(Event::Empty(start))?;
            }
            other => writer.write_event(other)?,
        }
    }
    let out = String::from_utf8(writer.into_inner()).context("rewritten file is not UTF-8")?;
    Ok((out, stats))
}

fn renumber_tag<'a>(
    tag: BytesStart<'a>,
    plan: &Renumber,
    stats: &mut RenumberStats,
) -> Result<BytesStart<'a>> {
    let element = tag.local_name().as_ref().to_string();
    let mut attrs: Vec<(String, String, String)> = Vec::new(); // key, raw value, unescaped
    for attr in tag.attributes() {
        let attr = attr.context("bad attribute in the bindings file")?;
        let raw = attr.value.to_string();
        let value = quick_xml::escape::unescape(&raw).map(|v| v.into_owned()).unwrap_or_else(|_| raw.clone());
        attrs.push((attr.key.as_ref().to_string(), raw, value));
    }
    let get = |k: &str| attrs.iter().find(|a| a.0 == k).map(|a| a.2.as_str());

    let mut changes: Vec<(&str, String)> = Vec::new();
    if let Some((n, suffix)) = get("input").and_then(split_js_input)
        && let Some(&to) = plan.inputs.get(&n)
        && to != n
    {
        changes.push(("input", format!("js{to}_{suffix}")));
        stats.inputs_changed += 1;
    }
    let instance = get("instance").and_then(|i| i.trim().parse::<u32>().ok());
    if let Some(n) = instance {
        if element == "options" && get("type") == Some("joystick") {
            if let Some(product) = get("Product") {
                let key = DeviceId::parse(product).key();
                if let Some(&to) = plan.options.get(&key)
                    && to != n
                {
                    changes.push(("instance", to.to_string()));
                    stats.options_changed += 1;
                }
            }
        } else if element == "joystick"
            && let Some(&to) = plan.inputs.get(&n)
            && to != n
        {
            changes.push(("instance", to.to_string()));
        }
    }
    if changes.is_empty() {
        return Ok(tag);
    }

    // Edit the raw tag text in place so spacing and quoting stay as they were.
    let mut raw: String = (*tag).to_string();
    for (key, new_value) in &changes {
        let old_raw = &attrs.iter().find(|a| a.0 == *key).expect("attribute exists").1;
        let mut done = false;
        for quote in ['"', '\''] {
            let needle = format!("{key}={quote}{old_raw}{quote}");
            if let Some(pos) = find_attribute(&raw, &needle) {
                let replacement = format!("{key}={quote}{}{quote}", escape_attr(new_value));
                raw.replace_range(pos..pos + needle.len(), &replacement);
                done = true;
                break;
            }
        }
        if !done {
            bail!("could not rewrite attribute {key} in <{element}>");
        }
    }
    let name_len = tag.name().as_ref().len();
    Ok(BytesStart::from_content(raw, name_len))
}

/// Find `needle` where it starts a whole attribute (preceded by whitespace).
fn find_attribute(raw: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = raw[from..].find(needle) {
        let pos = from + i;
        if pos > 0 && raw[..pos].ends_with(char::is_whitespace) {
            return Some(pos);
        }
        from = pos + 1;
    }
    None
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_inputs() {
        assert_eq!(split_js_input("js3_rctrl+button21"), Some((3, "rctrl+button21")));
        assert_eq!(split_js_input("js12_ "), Some((12, " ")));
        assert_eq!(split_js_input("kb1_lctrl+f"), None);
        assert_eq!(split_js_input("jsx_1"), None);
    }

    #[test]
    fn swaps_without_chaining() {
        let xml = r#"<a><rebind input="js1_x"/><rebind input="js2_y"/><rebind input="js3_z"/></a>"#;
        let plan =
            Renumber { inputs: [(1, 2), (2, 3), (3, 1)].into_iter().collect(), options: HashMap::new() };
        let (out, stats) = renumber(xml, &plan).unwrap();
        assert_eq!(out, r#"<a><rebind input="js2_x"/><rebind input="js3_y"/><rebind input="js1_z"/></a>"#);
        assert_eq!(stats.inputs_changed, 3);
    }

    #[test]
    fn keeps_spacing_inside_changed_tags() {
        let xml = "<options type=\"joystick\"  instance=\"2\" Product=\"Stick  {0126231D-0000-0000-0000-504944564944}\" />";
        let key = DeviceId::parse("Stick {0126231D-0000-0000-0000-504944564944}").key();
        let plan = Renumber { inputs: BTreeMap::new(), options: [(key, 4)].into_iter().collect() };
        let (out, _) = renumber(xml, &plan).unwrap();
        assert_eq!(
            out,
            "<options type=\"joystick\"  instance=\"4\" Product=\"Stick  {0126231D-0000-0000-0000-504944564944}\" />"
        );
    }

    #[test]
    fn refuses_two_devices_on_one_number() {
        let plan = Renumber { inputs: [(1, 2), (2, 2)].into_iter().collect(), options: HashMap::new() };
        assert!(renumber("<a/>", &plan).is_err());
    }
}
