//! Resources and prompts: the skills that carry the design judgement.

use serde_json::{json, Value};

/// (name, title, markdown). The same files ship in `skills/` as Agent Skills.
const SKILLS: &[(&str, &str, &str)] = &[
    (
        "kata-poster",
        "Making posters with Kata",
        include_str!("../../../skills/kata-poster/SKILL.md"),
    ),
    (
        "kata-cutout",
        "Clean cutouts",
        include_str!("../../../skills/kata-cutout/SKILL.md"),
    ),
    (
        "kata-print",
        "Printing in brand inks",
        include_str!("../../../skills/kata-print/SKILL.md"),
    ),
    (
        "kata-review",
        "Review before you show it",
        include_str!("../../../skills/kata-review/SKILL.md"),
    ),
    (
        "kata-brand",
        "A brand file Kata can use",
        include_str!("../../../skills/kata-brand/SKILL.md"),
    ),
];

const BRAND_EXAMPLE: &str = include_str!("../../../examples/brand.example.json");
const SCENE_EXAMPLE: &str = include_str!("../../../examples/scene.example.json");

pub fn list() -> Vec<Value> {
    let mut out: Vec<Value> = SKILLS
        .iter()
        .map(|(name, title, _)| json!({"uri": format!("kata://skills/{name}"), "name": name, "title": title, "mimeType": "text/markdown"}))
        .collect();
    out.push(json!({"uri": "kata://examples/brand", "name": "brand.example.json", "title": "Example brand file", "mimeType": "application/json"}));
    out.push(json!({"uri": "kata://examples/scene", "name": "scene.example.json", "title": "Example poster scene", "mimeType": "application/json"}));
    out.push(json!({"uri": "kata://targets", "name": "targets", "title": "Export targets and safe areas", "mimeType": "application/json"}));
    out
}

pub fn read(uri: &str) -> Option<Value> {
    if let Some(name) = uri.strip_prefix("kata://skills/") {
        let (_, _, body) = SKILLS.iter().find(|(n, _, _)| *n == name)?;
        return Some(json!({"uri": uri, "mimeType": "text/markdown", "text": body}));
    }
    let (mime, text) = match uri {
        "kata://examples/brand" => ("application/json", BRAND_EXAMPLE.to_owned()),
        "kata://examples/scene" => ("application/json", SCENE_EXAMPLE.to_owned()),
        "kata://targets" => (
            "application/json",
            serde_json::to_string_pretty(kata_core::targets::TARGETS).ok()?,
        ),
        _ => return None,
    };
    Some(json!({"uri": uri, "mimeType": mime, "text": text}))
}

pub fn prompts() -> Vec<Value> {
    vec![json!({
        "name": "poster",
        "title": "Make a poster",
        "description": "Brief-to-posting workflow for one poster from a photo.",
        "arguments": [
            {"name": "photo", "description": "Path to the photo", "required": true},
            {"name": "brand", "description": "Path to brand.json", "required": false},
            {"name": "message", "description": "The one thing this poster should say", "required": false},
            {"name": "target", "description": "Export target, e.g. ig-portrait", "required": false}
        ]
    })]
}

pub fn prompt(name: &str, args: &Value) -> Option<Value> {
    if name != "poster" {
        return None;
    }
    let get = |k: &str, d: &str| args.get(k).and_then(Value::as_str).unwrap_or(d).to_owned();
    let text = format!(
        "Make a poster from {photo}{brand}. Message: {message}. Target: {target}.\n\n\
         1. Read kata://skills/kata-poster and follow it step by step.\n\
         2. kata_status, then kata_brand if a brand file was given.\n\
         3. Write the concept and copy before touching pixels.\n\
         4. kata_segment (box hint if the scene is busy), look with kata_view, edit only what is wrong.\n\
         5. kata_print with the brand's inks and the poster's ground.\n\
         6. kata_compose with target {target}; fix every error in the review.\n\
         7. kata_view the logo, every headline edge and the subject's outline at zoom 2, and the whole poster with boost.\n\
         8. Report the file, the caption, and anything that needs a human (consent, credit).",
        photo = get("photo", "the photo"),
        brand = args.get("brand").and_then(Value::as_str).map(|b| format!(" using the brand in {b}")).unwrap_or_default(),
        message = get("message", "(ask, or infer from the brand)"),
        target = get("target", "ig-portrait"),
    );
    Some(
        json!({"description": "Poster workflow", "messages": [{"role": "user", "content": {"type": "text", "text": text}}]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_resource_can_be_read() {
        for r in list() {
            let uri = r["uri"].as_str().unwrap();
            assert!(read(uri).is_some(), "{uri}");
        }
        assert!(read("kata://nothing").is_none());
    }

    #[test]
    fn skills_have_frontmatter_with_name_and_description() {
        for (name, _, body) in SKILLS {
            // A Windows checkout may have CRLF line endings.
            let body = body.replace("\r\n", "\n");
            assert!(body.starts_with("---\n"), "{name}");
            assert!(body.contains(&format!("name: {name}\n")), "{name}");
            assert!(body.contains("\ndescription: "), "{name}");
        }
    }

    #[test]
    fn the_example_files_parse() {
        kata_core::brand::Brand::from_json(BRAND_EXAMPLE).unwrap();
        let _: kata_engine::compose::Scene = serde_json::from_str(SCENE_EXAMPLE).unwrap();
    }
}
