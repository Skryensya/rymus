// The equalizer: ten bands in mpv's audio filters, preset profiles and your own. The filter is put in once, flat, and
// the bands are then turned with commands: replacing an audio filter is heard as a gap, turning a band is not.
use serde_json::{json, Value};

pub const FREQS: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
pub const LIMIT: i64 = 12; // dB either way
pub type Gains = [i64; 10];

pub struct Preset {
    pub name: String,
    pub gains: Gains,
    pub custom: bool,
    pub base: String, // the built-in one yours was made from: what reset goes back to
}

const BUILT_IN: [(&str, Gains); 10] = [
    ("Flat", [0; 10]),
    ("Bass boost", [6, 5, 4, 2, 0, 0, 0, 0, 0, 0]),
    ("Bass cut", [-6, -5, -3, -1, 0, 0, 0, 0, 0, 0]),
    ("Treble boost", [0, 0, 0, 0, 0, 0, 1, 3, 5, 6]),
    ("Vocal", [-2, -2, -1, 1, 3, 4, 3, 1, 0, -1]),
    ("Rock", [4, 3, 2, 0, -1, -1, 1, 2, 3, 4]),
    ("Pop", [-1, 1, 3, 4, 3, 0, -1, -1, 1, 2]),
    ("Electronic", [5, 4, 1, 0, -2, 1, 0, 1, 4, 5]),
    ("Acoustic", [3, 3, 2, 1, 1, 1, 2, 2, 2, 1]),
    ("Loudness", [5, 3, 0, 0, -1, 0, 0, 0, 3, 4]),
];

// Built-in profiles, then yours as saved ("eq_presets": [{"name", "gains"}]).
pub fn presets(saved: Option<&str>) -> Vec<Preset> {
    let mut all: Vec<Preset> = BUILT_IN.iter().map(|(n, g)| Preset { name: n.to_string(), gains: *g, custom: false, base: n.to_string() }).collect();
    let mine: Value = saved.and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null);
    for p in mine.as_array().into_iter().flatten() {
        let (Some(name), Some(gains)) = (p["name"].as_str(), p["gains"].as_array()) else { continue };
        let mut g = [0; 10];
        for (slot, v) in g.iter_mut().zip(gains) {
            *slot = v.as_i64().unwrap_or(0).clamp(-LIMIT, LIMIT);
        }
        if !name.trim().is_empty() && !all.iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
            // older ones did not say: the copies once made on editing were called "My Pop", "My Pop 2"
            let copied = name.strip_prefix("My ").map(|n| n.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end());
            let copied = copied.filter(|c| BUILT_IN.iter().any(|b| b.0 == *c));
            let base = crate::item::clean(p["from"].as_str().or(copied).unwrap_or("Flat"));
            all.push(Preset { name: crate::item::clean(name), gains: g, custom: true, base });
        }
    }
    all
}
// The gains of a built-in profile; flat for a name that is none.
pub fn built_in(name: &str) -> Gains {
    BUILT_IN.iter().find(|p| p.0 == name).map_or([0; 10], |p| p.1)
}
pub fn save(presets: &[Preset]) -> String {
    Value::Array(presets.iter().filter(|p| p.custom).map(|p| json!({"name": p.name, "gains": p.gains, "from": p.base})).collect()).to_string()
}

// The filter as it is put in: always flat, so a new run finds it unchanged and leaves it alone.
pub fn filter() -> String {
    let graph = graph(&[0; 10], false);
    format!("@eq:lavfi=graph=%{}%{graph}", graph.len())
}
// The bands with their gains in place, for a player that is set up once and never changed (the crossfade's end).
pub fn graph(gains: &Gains, mono: bool) -> String {
    let headroom = gains.iter().copied().max().unwrap_or(0).max(0);
    let bands: Vec<String> = FREQS.iter().zip(gains).enumerate().map(|(i, (f, g))| format!("equalizer@b{i}=f={f}:t=o:w=1:g={g}")).collect();
    // last, the mono switch: extrastereo keeps this much of what differs between left and right (0: nothing, mono)
    format!("{},volume@pre=volume=-{headroom}dB,extrastereo@mono=m={}:c=0", bands.join(","), u8::from(!mono))
}
// The commands that set every band, and lower the whole by the biggest boost so a boost never clips.
pub fn commands(gains: &Gains, mono: bool) -> Vec<Value> {
    let headroom = gains.iter().copied().max().unwrap_or(0).max(0);
    let mut out: Vec<Value> = gains.iter().enumerate().map(|(i, g)| json!(["af-command", "eq", "gain", g.to_string(), format!("equalizer@b{i}")])).collect();
    out.push(json!(["af-command", "eq", "volume", format!("-{headroom}dB"), "volume@pre"]));
    out.push(json!(["af-command", "eq", "m", if mono { "0" } else { "1" }, "extrastereo@mono"]));
    out
}

// What each band moves, said for someone who never used an equalizer.
pub const BANDS: [(&str, &str); 10] = [
    ("Sub-bass", "the rumble you feel more than hear: deep kicks and synth bass"),
    ("Bass", "the weight of the kick drum and the bass guitar"),
    ("Upper bass", "warmth and body; too much sounds boomy"),
    ("Low mids", "the fullness of guitars and pianos; too much sounds muddy"),
    ("Mids", "the body of voices and snares; too much sounds boxy"),
    ("Mids", "where most instruments sit; more brings them forward"),
    ("Upper mids", "attack and bite; too much sounds harsh"),
    ("Presence", "the clarity of voices and consonants"),
    ("Treble", "cymbals and sparkle; too much makes the s sounds hiss"),
    ("Air", "openness and shimmer at the very top"),
];
// The three ranges the bands fall in, and how many bands each takes, low to high.
pub const REGIONS: [(&str, usize); 3] = [("bass", 4), ("mids", 3), ("treble", 3)];

// A profile in words: which ranges it raises and which it lowers.
pub fn describe(g: &Gains) -> String {
    let (mut more, mut less, mut at) = (vec![], vec![], 0);
    for (name, n) in REGIONS {
        let name = crate::i18n::t(name);
        let avg = g[at..at + n].iter().sum::<i64>() as f64 / n as f64;
        at += n;
        if avg >= 1.5 {
            more.push(name);
        } else if avg <= -1.5 {
            less.push(name);
        }
    }
    let list = |v: &[&str]| match v {
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    };
    match (more.is_empty(), less.is_empty()) {
        (true, true) if g.iter().all(|&x| x == 0) => "The music as it was mixed: nothing raised, nothing lowered".into(),
        (true, true) => "Small touches: nothing raised or lowered by much".into(),
        (false, true) => format!("More {}", list(&more)),
        (true, false) => format!("Less {}", list(&less)),
        (false, false) => format!("More {}, less {}", list(&more), list(&less)),
    }
}

pub fn freq_label(f: u32) -> String {
    if f >= 1000 { format!("{}k", f / 1000) } else { f.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_load_save_and_commands() {
        let saved = r#"[{"name":"Mine","gains":[20,1,2]},{"name":"flat","gains":[]},{"name":"","gains":[1]},{"nope":1}]"#;
        let all = presets(Some(saved));
        let mine = all.iter().find(|p| p.custom).unwrap();
        assert_eq!((mine.name.as_str(), mine.gains[0], mine.gains[2], mine.gains[9]), ("Mine", 12, 2, 0), "clamped and padded");
        assert_eq!(all.iter().filter(|p| p.custom).count(), 1, "a name already taken, or empty, is skipped");
        assert_eq!(presets(Some(&save(&all))).len(), all.len());
        let old = presets(Some(r#"[{"name":"My Pop 2","gains":[]},{"name":"My own","gains":[]}]"#));
        assert_eq!(old.iter().filter(|p| p.custom).map(|p| p.base.as_str()).collect::<Vec<_>>(), ["Pop", "Flat"]);
        assert_eq!(presets(Some("garbage")).len(), BUILT_IN.len());
        let c = commands(&[3, -2, 0, 0, 0, 0, 0, 0, 0, 6], false);
        assert_eq!(c[0], json!(["af-command", "eq", "gain", "3", "equalizer@b0"]));
        assert_eq!(c[10], json!(["af-command", "eq", "volume", "-6dB", "volume@pre"]));
        assert_eq!(commands(&[-3; 10], false)[10][3], "-0dB");
        assert_eq!(commands(&[0; 10], true)[11], json!(["af-command", "eq", "m", "0", "extrastereo@mono"]));
        assert!(graph(&[3, -2, 0, 0, 0, 0, 0, 0, 0, 6], true).ends_with("equalizer@b9=f=16000:t=o:w=1:g=6,volume@pre=volume=-6dB,extrastereo@mono=m=0:c=0"));
        assert_eq!(describe(&[0; 10]), "The music as it was mixed: nothing raised, nothing lowered");
        assert_eq!(describe(&[4, 3, 2, 0, -1, -1, 1, 2, 3, 4]), "More bass and treble"); // Rock
        assert_eq!(describe(&[-6, -5, -3, -1, 0, 0, 0, 0, 0, 0]), "Less bass");
        assert_eq!(describe(&[3, 3, 3, 3, -3, -3, -3, 3, 3, 3]), "More bass and treble, less mids");
        let f = filter();
        assert!(f.starts_with("@eq:lavfi=graph=%") && f.contains("equalizer@b9=f=16000") && !f.contains("g=3"));
    }
}
