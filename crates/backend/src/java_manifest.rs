use rustc_hash::FxHashMap;

pub fn parse_java_manifest(string: &str) -> FxHashMap<String, String> {
    let mut map = FxHashMap::default();

    let mut last_pair: Option<(String, String)> = None;
    for line in string.lines() {
        if line.starts_with(' ') {
            if let Some((_, last_value)) = &mut last_pair {
                last_value.push_str(line.trim_ascii_start());
            }
        } else {
            if let Some(last_pair) = last_pair.take() {
                map.insert(last_pair.0, last_pair.1);
            }

            if let Some((key, value)) = line.split_once(':') {
                last_pair = Some((key.to_string(), value.trim_ascii_start().to_string()));
            }
        }
    }

    if let Some(last_pair) = last_pair.take() {
        map.insert(last_pair.0, last_pair.1);
    }

    map
}
