//! Compiles a JSON Schema into a Nickel contract.
//!
//! The schema (`schema/plate.schema.json`) is the single definition of the configuration
//! format. Compiling it to a Nickel contract means Nickel itself checks every value, fills in
//! defaults and reports errors at the exact source position. The same JSON drives the editor's
//! hover docs and completions.
//!
//! Supported JSON Schema (2020-12) subset:
//!
//! | Keyword                                         | Nickel                                       |
//! |-------------------------------------------------|----------------------------------------------|
//! | `type` (string, number, integer, boolean, object, array) | type checks / record or array contract |
//! | `properties`, `required`, `additionalProperties: false` | record contract, closed or open with `..` |
//! | `additionalProperties: {schema}` (without `properties`) | dictionary contract `{ _ \| C }`         |
//! | `description`                                   | `\| doc "…"`                                  |
//! | `default`                                       | filled in after evaluation ([`apply_defaults`]) |
//! | `enum`, `const` (strings)                       | accepts enum tags (`'metric`) or strings     |
//! | `type: string`                                  | a string or an enum tag (both export as JSON strings) |
//! | `pattern`, `minimum`, `maximum`, `exclusive*`   | predicates                                   |
//! | `items`, `prefixItems`, `minItems`, `maxItems`  | `Array C` plus a length check (tuples such as `[x, y]` must use one schema for every position) |
//! | `anyOf`, `oneOf`                                | combined predicate, a tagged union when every alternative fixes the same property with `const` (e.g. `rule`), or else the first alternative whose immediate check passes |
//! | `$ref: "#/$defs/…"`, `$defs`                    | shared, named contracts                      |
//! | `errorMessage` (non-standard)                   | the message shown when the value is rejected |
//!
//! Anything else is rejected with an error rather than silently ignored.

use std::collections::BTreeMap;
use std::fmt::Write;

use serde_json::{Map, Value};

/// Keywords that carry no validation meaning.
const METADATA: &[&str] =
    &["$schema", "$id", "$comment", "title", "description", "examples", "default", "errorMessage", "$defs"];

/// Helper contracts used by the generated code.
// Note: Nickel records are recursive, so a parameter must not share its name with a field of
// a record literal that uses it (`{ message = message }` would refer to itself).
const PRELUDE: &str = r#"{
    name_of = fun v => if std.is_enum v then std.string.from_enum v else if std.is_string v then v else "",
    in_set = fun values v => (std.is_enum v || std.is_string v) && std.array.elem (name_of v) values,
    check = fun msg pred =>
      std.contract.from_validator (fun v => if pred v then 'Ok else 'Error { message = msg }),
    tagged = fun prop variants msg =>
      std.contract.custom (fun label value =>
        if !(std.is_record value) then
          'Error { message = msg }
        else if !(std.record.has_field prop value) then
          'Error { message = "missing `%{prop}`: %{msg}" }
        else
          let tag = name_of (std.record.get prop value) in
          if std.record.has_field tag variants then
            'Ok (std.contract.apply (std.record.get tag variants) label value)
          else
            'Error { message = "unknown %{prop} '%{tag}: %{msg}" }
      ),
    either = fun contracts msg =>
      std.contract.custom (fun label value =>
        let rec go = fun rest =>
          if std.array.length rest == 0 then
            'Error { message = msg }
          else
            let result = std.contract.check (std.array.first rest) label value in
            result |> match { 'Ok _ => result, _ => go (std.array.drop_first rest) }
        in
        go contracts
      ),
    by_kind = fun has_record record_c has_array array_c others msg =>
      std.contract.custom (fun label value =>
        if has_record && std.is_record value then
          'Ok (std.contract.apply record_c label value)
        else if has_array && std.is_array value then
          'Ok (std.contract.apply array_c label value)
        else
          let result = std.contract.check (either others msg) label value in
          result |> match { 'Ok _ => result, _ => 'Error { message = msg } }
      ),
    service_fields = ["parts", "place", "connect", "claims", "scope", "params", "media", "feeds"],
    plan_fields = ["start", "spaces", "rules", "needs", "preview", "goals", "engine", "grid", "assemblies", "services"],
    is_library = fun v =>
      std.is_record v && !(std.array.any (fun f => std.record.has_field f v) plan_fields),
    is_service = fun v => is_library v && std.array.any (fun f => std.record.has_field f v) service_fields,
    is_assemblies = fun v =>
      is_library v
      && std.array.length (std.record.fields v) > 0
      && std.array.all
        (fun f =>
          let x = std.record.get f v in
          std.is_record x && (std.record.has_field "thickness" x || std.record.has_field "layers" x)
        )
        (std.record.fields v),
    by_shape = fun plate_c plan_c service_c assemblies_c =>
      std.contract.custom (fun label value =>
        if std.is_record value && std.record.has_field "start" value then
          'Ok (std.contract.apply plate_c label value)
        else if is_service value then
          'Ok (std.contract.apply service_c label value)
        else if is_assemblies value then
          'Ok (std.contract.apply assemblies_c label value)
        else
          'Ok (std.contract.apply plan_c label value)
      ),
    sized_array = fun item min max msg =>
      std.contract.custom (fun label value =>
        if !(std.is_array value) then
          'Error { message = msg }
        else
          let n = std.array.length value in
          if n < min || (max >= 0 && n > max) then
            'Error { message = msg }
          else
            'Ok (value | Array item)
      ),
  }"#;

/// The compiled schema: a Nickel expression evaluating to the root contract.
#[derive(Debug, Clone)]
pub struct CompiledSchema {
    pub nickel: String,
}

pub fn compile(schema: &Value) -> Result<CompiledSchema, String> {
    let mut c = Compiler { root: schema, contracts: Vec::new(), refs: BTreeMap::new() };
    let root = c.contract(schema, "#")?;
    Ok(c.finish(&root))
}

/// The contract for a user's main file, by its shape: the schema's root (a plate) when the file
/// has a `start`; a `Service` or `Assemblies` for a library file that is one (it has a service's
/// fields, or every field is an assembly); else the definition `plan` (a plan file, shown on its
/// own). They share one set of compiled definitions. See [`FileKind`].
pub fn compile_file(schema: &Value, plan: &str) -> Result<CompiledSchema, String> {
    let mut c = Compiler { root: schema, contracts: Vec::new(), refs: BTreeMap::new() };
    let plate = c.contract(schema, "#")?;
    let mut def = |name: &str| {
        let reference = format!("#/$defs/{name}");
        c.contract(&serde_json::json!({ "$ref": reference }), &reference)
    };
    let (plan, service, assemblies) = (def(plan)?, def("Service")?, def("Assemblies")?);
    Ok(c.finish(&format!("__plate.by_shape {plate} {plan} {service} {assemblies}")))
}

/// What a main file is, by the shape of its value (the same tests as the `by_shape` contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    /// It has a `start`: something to draw.
    Plate,
    /// A plan, to import into a space; drawn on its own in a slot.
    Plan,
    /// A service, to put in a plate's `services`.
    Service,
    /// A set of assemblies, for a plate's `assemblies`.
    Assemblies,
}

const SERVICE_FIELDS: &[&str] = &["parts", "place", "connect", "claims", "scope", "params", "media", "feeds"];
const PLAN_FIELDS: &[&str] =
    &["start", "spaces", "rules", "needs", "preview", "goals", "engine", "grid", "assemblies", "services"];

impl FileKind {
    pub fn of(value: &Value) -> FileKind {
        let Some(obj) = value.as_object() else { return FileKind::Plan };
        if obj.contains_key("start") {
            return FileKind::Plate;
        }
        if PLAN_FIELDS.iter().any(|f| obj.contains_key(*f)) {
            return FileKind::Plan;
        }
        if SERVICE_FIELDS.iter().any(|f| obj.contains_key(*f)) {
            return FileKind::Service;
        }
        let assemblies = !obj.is_empty()
            && obj.values().all(|v| v.get("thickness").is_some() || v.get("layers").is_some());
        if assemblies {
            FileKind::Assemblies
        } else {
            FileKind::Plan
        }
    }

    /// The schema definition its value is checked against (the root for a plate).
    pub fn definition(self) -> Option<&'static str> {
        match self {
            FileKind::Plate => None,
            FileKind::Plan => Some("Plan"),
            FileKind::Service => Some("Service"),
            FileKind::Assemblies => Some("Assemblies"),
        }
    }
}

impl Compiler<'_> {
    fn finish(self, root: &str) -> CompiledSchema {
        let mut out = String::new();
        out.push_str("let __plate = ");
        out.push_str(PRELUDE);
        out.push_str(" in\nlet rec __c = {\n");
        for (key, body) in &self.contracts {
            let _ = writeln!(out, "  {key} = {body},");
        }
        let _ = write!(out, "}} in\n{root}");
        CompiledSchema { nickel: out }
    }
}

/// The discriminating property and each alternative by its tag.
type TaggedUnion = (String, Vec<(String, Value)>);

struct Compiler<'a> {
    root: &'a Value,
    /// Generated contracts as (key, body), referenced as `__c.<key>`: `$defs` keep their schema
    /// name, anonymous ones are numbered.
    contracts: Vec<(String, String)>,
    /// `$ref` target → contract reference, so shared definitions compile once.
    refs: BTreeMap<String, String>,
}

impl Compiler<'_> {
    /// Register a contract body and return a reference to it.
    fn add(&mut self, body: String) -> String {
        let key = format!("c{}", self.contracts.len());
        self.contracts.push((key.clone(), body));
        format!("__c.{key}")
    }

    fn resolve<'s>(&'s self, reference: &str) -> Result<&'s Value, String> {
        let name = reference
            .strip_prefix("#/$defs/")
            .ok_or_else(|| format!("unsupported $ref `{reference}`: only #/$defs/... is supported"))?;
        self.root
            .get("$defs")
            .and_then(|d| d.get(name))
            .ok_or_else(|| format!("$ref `{reference}` not found"))
    }

    /// Follow `$ref`s to the schema that actually describes the value.
    fn deref<'s>(&'s self, mut node: &'s Value) -> Result<&'s Value, String> {
        for _ in 0..32 {
            match node.get("$ref").and_then(Value::as_str) {
                Some(r) => node = self.resolve(r)?,
                None => return Ok(node),
            }
        }
        Err("$ref cycle".into())
    }

    /// A reference to a contract for `node`.
    fn contract(&mut self, node: &Value, at: &str) -> Result<String, String> {
        let obj = node.as_object().ok_or_else(|| format!("{at}: a schema must be an object"))?;
        check_keywords(obj, at)?;

        if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
            if let Some(existing) = self.refs.get(reference) {
                return Ok(existing.clone());
            }
            // Reserve a slot named after the definition first, so recursive definitions can
            // refer to themselves.
            let name = reference.trim_start_matches("#/$defs/");
            let key = field_name(&format!("def_{name}"));
            let index = self.contracts.len();
            self.contracts.push((key.clone(), String::new()));
            let slot = format!("__c.{key}");
            self.refs.insert(reference.to_owned(), slot.clone());
            let target = self.resolve(reference)?.clone();
            let body = self.contract(&target, reference)?;
            // A definition that compiled to a single anonymous contract: inline it.
            match body.strip_prefix("__c.").and_then(|k| self.contracts.iter().position(|(key, _)| key == k))
            {
                Some(i) if i == self.contracts.len() - 1 && i > index => {
                    let (_, inner) = self.contracts.pop().expect("last contract");
                    self.contracts[index].1 = inner;
                }
                _ => self.contracts[index].1 = body,
            }
            return Ok(slot);
        }

        if let Some(pred) = self.predicate(node, at)? {
            let message = error_message(node).unwrap_or_else(|| describe(node));
            return Ok(self.add(format!("__plate.check {} ({pred})", nickel_string(&message))));
        }

        if is_object(obj) {
            return self.record(obj, at);
        }

        if obj.get("type").and_then(Value::as_str) == Some("array") {
            return self.array(node, obj, at);
        }

        for key in ["oneOf", "anyOf"] {
            if let Some(alternatives) = obj.get(key).and_then(Value::as_array) {
                if let Some((prop, variants)) = self.tagged_union(alternatives)? {
                    let tags: Vec<String> = variants.iter().map(|(t, _)| format!("'{t}")).collect();
                    let message = error_message(node)
                        .unwrap_or_else(|| format!("expected `{prop}` to be one of {}", tags.join(", ")));
                    let mut entries = String::new();
                    for (tag, alt) in variants {
                        let r = self.contract(&alt, &format!("{at}/{key}"))?;
                        let _ = write!(entries, "{} = {r}, ", field_name(&tag));
                    }
                    return Ok(self.add(format!(
                        "__plate.tagged {} {{ {entries}}} {}",
                        nickel_string(&prop),
                        nickel_string(&message)
                    )));
                }
                let message = error_message(node).unwrap_or_else(|| "unexpected value".to_owned());
                // Dispatch on the kind of value when that picks one alternative, so a record or
                // a list reports its own precise error (an unknown field, a bad rule kind).
                let kinds: Vec<&str> =
                    alternatives.iter().map(|a| self.kind_of(a)).collect::<Result<_, _>>()?;
                let count = |k: &str| kinds.iter().filter(|x| **x == k).count();
                if count("record") <= 1 && count("array") <= 1 && count("other") < alternatives.len() {
                    let mut record = None;
                    let mut array = None;
                    let mut others = Vec::new();
                    for (i, (alt, kind)) in alternatives.iter().zip(&kinds).enumerate() {
                        let r = self.contract(alt, &format!("{at}/{key}/{i}"))?;
                        match *kind {
                            "record" => record = Some(r),
                            "array" => array = Some(r),
                            _ => others.push(r),
                        }
                    }
                    return Ok(self.add(format!(
                        "__plate.by_kind {} ({}) {} ({}) [{}] {}",
                        record.is_some(),
                        record.unwrap_or_else(|| "Dyn".into()),
                        array.is_some(),
                        array.unwrap_or_else(|| "Dyn".into()),
                        others.join(", "),
                        nickel_string(&message)
                    )));
                }
                let refs = alternatives
                    .iter()
                    .enumerate()
                    .map(|(i, a)| self.contract(a, &format!("{at}/{key}/{i}")))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(self.add(format!(
                    "__plate.either [{}] {}",
                    refs.join(", "),
                    nickel_string(&message)
                )));
            }
        }

        Ok("Dyn".into())
    }

    /// Arrays: `items` (every element), or `prefixItems` (a tuple such as `[x, y]`; all positions
    /// must share one schema), with `minItems` / `maxItems`.
    fn array(&mut self, node: &Value, obj: &Map<String, Value>, at: &str) -> Result<String, String> {
        let item = match (obj.get("prefixItems").and_then(Value::as_array), obj.get("items")) {
            (Some(prefix), items) => {
                if items.is_some_and(|i| i != &Value::Bool(false)) {
                    return Err(format!(
                        "{at}: `prefixItems` with `items` other than false is not supported"
                    ));
                }
                let first = prefix.first().ok_or_else(|| format!("{at}: empty `prefixItems`"))?;
                if prefix.iter().any(|p| p != first) {
                    return Err(format!("{at}: `prefixItems` must all use the same schema"));
                }
                self.contract(first, &format!("{at}/prefixItems/0"))?
            }
            (None, Some(Value::Bool(true)) | None) => "Dyn".into(),
            (None, Some(items)) => self.contract(items, &format!("{at}/items"))?,
        };
        let fixed = obj.get("prefixItems").and_then(Value::as_array).map(Vec::len);
        let min = obj.get("minItems").and_then(Value::as_u64).or(fixed.map(|n| n as u64)).unwrap_or(0);
        let max = obj
            .get("maxItems")
            .and_then(Value::as_u64)
            .map(|n| n as i64)
            .or(fixed.map(|n| n as i64))
            .unwrap_or(-1);
        let message = error_message(node).unwrap_or_else(|| match (min, max) {
            (0, -1) => "expected an array".to_owned(),
            (min, -1) => format!("expected an array of at least {min} items"),
            (min, max) if min as i64 == max => format!("expected an array of {min} items"),
            (min, max) => format!("expected an array of {min} to {max} items"),
        });
        Ok(self.add(format!("__plate.sized_array {item} {min} ({max}) {}", nickel_string(&message))))
    }

    fn record(&mut self, obj: &Map<String, Value>, at: &str) -> Result<String, String> {
        let required: Vec<&str> = obj
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if let Some(values) = obj.get("additionalProperties").filter(|v| v.is_object()) {
            if obj.contains_key("properties") {
                return Err(format!(
                    "{at}: `additionalProperties` with a schema can't be combined with `properties`"
                ));
            }
            let contract = self.contract(values, &format!("{at}/additionalProperties"))?;
            return Ok(self.add(format!("{{ _ | {contract} }}")));
        }
        let mut body = String::from("{ ");
        if let Some(props) = obj.get("properties").and_then(Value::as_object) {
            for (name, prop) in props {
                let contract = self.contract(prop, &format!("{at}/properties/{name}"))?;
                let _ = write!(body, "{} | {contract}", field_name(name));
                if let Some(doc) = prop.get("description").and_then(Value::as_str) {
                    let _ = write!(body, " | doc {}", nickel_string(doc));
                }
                // Schema defaults are filled in after evaluation (see `apply_defaults`), not with
                // `| default`: two `| default` values for one field are a merge conflict in
                // Nickel, which would break files that cascade with their own defaults.
                if !required.contains(&name.as_str()) {
                    body.push_str(" | optional");
                }
                body.push_str(", ");
            }
        }
        if obj.get("additionalProperties") != Some(&Value::Bool(false)) {
            body.push_str("..");
        }
        body.push('}');
        Ok(self.add(body))
    }

    /// If every alternative is an object pinning the same property to a constant string,
    /// return that property and the alternatives by tag.
    fn tagged_union(&self, alternatives: &[Value]) -> Result<Option<TaggedUnion>, String> {
        let resolved: Vec<&Value> = alternatives.iter().map(|a| self.deref(a)).collect::<Result<_, _>>()?;
        let Some(first) = resolved.first().and_then(|v| v.get("properties")).and_then(Value::as_object)
        else {
            return Ok(None);
        };
        'candidates: for prop in first.keys() {
            let mut variants = Vec::new();
            for (alt, original) in resolved.iter().zip(alternatives) {
                let Some(tag) = alt.get("properties").and_then(|p| p.get(prop)).and_then(const_string) else {
                    continue 'candidates;
                };
                variants.push((tag, original.clone()));
            }
            return Ok(Some((prop.clone(), variants)));
        }
        Ok(None)
    }

    /// What kind of value a schema describes: "record", "array" or "other" (scalars, mixes).
    fn kind_of(&self, node: &Value) -> Result<&'static str, String> {
        let node = self.deref(node)?;
        let Some(obj) = node.as_object() else { return Ok("other") };
        if is_object(obj) {
            return Ok("record");
        }
        if obj.get("type").and_then(Value::as_str) == Some("array") {
            return Ok("array");
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(alts) = obj.get(key).and_then(Value::as_array) {
                let kinds: Vec<&str> = alts.iter().map(|a| self.kind_of(a)).collect::<Result<_, _>>()?;
                if kinds.iter().all(|k| *k == "record") {
                    return Ok("record");
                }
            }
        }
        Ok("other")
    }

    /// A Nickel predicate (`fun v => …`) for schemas that only constrain scalar values.
    fn predicate(&self, node: &Value, at: &str) -> Result<Option<String>, String> {
        let node = self.deref(node)?;
        let Some(obj) = node.as_object() else { return Ok(None) };
        if is_object(obj) || obj.get("type").and_then(Value::as_str) == Some("array") {
            return Ok(None);
        }

        let mut checks: Vec<String> = Vec::new();

        if let Some(values) = obj.get("enum").and_then(Value::as_array) {
            checks.push(format!("__plate.in_set {} v", string_list(values, at)?));
        }
        if let Some(value) = obj.get("const") {
            checks.push(format!("__plate.in_set {} v", string_list(std::slice::from_ref(value), at)?));
        }

        match obj.get("type").and_then(Value::as_str) {
            Some("string") => checks.push("(std.is_string v || std.is_enum v)".into()),
            Some("number") => checks.push("std.is_number v".into()),
            Some("integer") => checks.push("(std.is_number v && std.number.is_integer v)".into()),
            Some("boolean") => checks.push("std.is_bool v".into()),
            Some(other) => return Err(format!("{at}: unsupported type `{other}`")),
            None => {}
        }

        if let Some(pattern) = obj.get("pattern").and_then(Value::as_str) {
            checks.push(format!(
                "((std.is_string v || std.is_enum v) && std.string.is_match {} (__plate.name_of v))",
                nickel_string(pattern)
            ));
        }
        for (key, op) in
            [("minimum", ">="), ("maximum", "<="), ("exclusiveMinimum", ">"), ("exclusiveMaximum", "<")]
        {
            if let Some(bound) = obj.get(key).and_then(Value::as_f64) {
                checks.push(format!("(std.is_number v && v {op} {})", number(bound)));
            }
        }

        let alternatives = obj.get("anyOf").or_else(|| obj.get("oneOf")).and_then(Value::as_array);
        if let Some(alternatives) = alternatives {
            let mut parts = Vec::new();
            for (i, alt) in alternatives.iter().enumerate() {
                match self.predicate(alt, &format!("{at}/anyOf/{i}"))? {
                    Some(p) => parts.push(format!("({p}) v")),
                    None => return Ok(None),
                }
            }
            checks.push(format!("({})", parts.join(" || ")));
        }

        if checks.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!("fun v => {}", checks.join(" && "))))
    }
}

fn is_object(obj: &Map<String, Value>) -> bool {
    obj.get("type").and_then(Value::as_str) == Some("object") || obj.contains_key("properties")
}

fn check_keywords(obj: &Map<String, Value>, at: &str) -> Result<(), String> {
    const SUPPORTED: &[&str] = &[
        "$ref",
        "type",
        "properties",
        "required",
        "additionalProperties",
        "enum",
        "const",
        "pattern",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "items",
        "prefixItems",
        "minItems",
        "maxItems",
        "anyOf",
        "oneOf",
    ];
    for key in obj.keys() {
        let known =
            SUPPORTED.contains(&key.as_str()) || METADATA.contains(&key.as_str()) || key.starts_with("x-");
        if !known {
            return Err(format!("{at}: unsupported JSON Schema keyword `{key}`"));
        }
    }
    Ok(())
}

fn const_string(node: &Value) -> Option<String> {
    if let Some(s) = node.get("const").and_then(Value::as_str) {
        return Some(s.to_owned());
    }
    match node.get("enum").and_then(Value::as_array).map(Vec::as_slice) {
        Some([Value::String(s)]) => Some(s.clone()),
        _ => None,
    }
}

fn error_message(node: &Value) -> Option<String> {
    node.get("errorMessage").and_then(Value::as_str).map(str::to_owned)
}

/// A short description of what a schema accepts, used when it has no `errorMessage`.
fn describe(node: &Value) -> String {
    if let Some(values) = node.get("enum").and_then(Value::as_array) {
        let names: Vec<String> = values.iter().filter_map(Value::as_str).map(|s| format!("'{s}")).collect();
        return format!("expected one of {}", names.join(", "));
    }
    if let Some(value) = node.get("const").and_then(Value::as_str) {
        return format!("expected '{value}");
    }
    let mut text = match node.get("type").and_then(Value::as_str) {
        Some("string") => "expected text".to_owned(),
        Some("number") => "expected a number".to_owned(),
        Some("integer") => "expected a whole number".to_owned(),
        Some("boolean") => "expected true or false".to_owned(),
        _ => "unexpected value".to_owned(),
    };
    for (key, words) in [
        ("minimum", "at least"),
        ("maximum", "at most"),
        ("exclusiveMinimum", "greater than"),
        ("exclusiveMaximum", "less than"),
    ] {
        if let Some(bound) = node.get(key).and_then(Value::as_f64) {
            let _ = write!(text, " {words} {}", number(bound));
        }
    }
    text
}

fn string_list(values: &[Value], at: &str) -> Result<String, String> {
    let items = values
        .iter()
        .map(|v| {
            v.as_str().map(nickel_string).ok_or_else(|| format!("{at}: only string enums are supported"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("[{}]", items.join(", ")))
}

fn number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn field_name(name: &str) -> String {
    let mut chars = name.chars();
    let ident = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '\'');
    if ident {
        name.to_owned()
    } else {
        nickel_string(name)
    }
}

/// A Nickel string literal.
pub fn nickel_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("\\%"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Fill in `default`s from the schema for fields the configuration leaves out, recursing into
/// nested objects (through `$ref`s and tagged unions). Runs on the evaluated, already-checked
/// data, like JSON Schema validators' "use defaults" mode.
pub fn apply_defaults(schema: &Value, value: &mut Value) {
    apply_defaults_at(schema, schema, value, 0);
}

/// [`apply_defaults`] for a value of one definition, such as `Plan`.
pub fn apply_defaults_def(schema: &Value, name: &str, value: &mut Value) {
    let node = serde_json::json!({ "$ref": format!("#/$defs/{name}") });
    apply_defaults_at(schema, &node, value, 0);
}

fn deref_in<'a>(root: &'a Value, mut node: &'a Value) -> &'a Value {
    for _ in 0..32 {
        match node.get("$ref").and_then(Value::as_str) {
            Some(r) => match r.strip_prefix("#/$defs/").and_then(|n| root.get("$defs")?.get(n)) {
                Some(target) => node = target,
                None => return node,
            },
            None => return node,
        }
    }
    node
}

fn apply_defaults_at(root: &Value, node: &Value, value: &mut Value, depth: usize) {
    if depth > 64 {
        return;
    }
    let node = deref_in(root, node);

    // Unions: continue with the alternative that matches, by its constant (tagged unions) or
    // else by the shape of the value.
    for key in ["oneOf", "anyOf"] {
        if let Some(alternatives) = node.get(key).and_then(Value::as_array) {
            let resolved: Vec<&Value> = alternatives.iter().map(|a| deref_in(root, a)).collect();
            let tagged = resolved.iter().find(|alt| {
                alt.get("properties").and_then(Value::as_object).is_some_and(|props| {
                    props.iter().any(|(name, p)| {
                        const_string(p)
                            .is_some_and(|tag| value.get(name).and_then(Value::as_str) == Some(&tag))
                    })
                })
            });
            let shaped = || {
                resolved.iter().find(|alt| match value {
                    Value::Object(_) => {
                        alt.get("type").and_then(Value::as_str) == Some("object")
                            || alt.get("properties").is_some()
                            || alt.get("oneOf").is_some()
                    }
                    Value::Array(_) => alt.get("type").and_then(Value::as_str) == Some("array"),
                    _ => false,
                })
            };
            if let Some(alt) = tagged.or_else(shaped) {
                apply_defaults_at(root, alt, value, depth + 1);
            }
            return;
        }
    }

    match value {
        Value::Array(items) => {
            if let Some(item) = node.get("items").filter(|i| i.is_object()) {
                for v in items {
                    apply_defaults_at(root, item, v, depth + 1);
                }
            }
        }
        Value::Object(obj) => {
            if let Some(props) = node.get("properties").and_then(Value::as_object) {
                for (name, prop) in props {
                    if !obj.contains_key(name) {
                        if let Some(default) = prop.get("default") {
                            obj.insert(name.clone(), default.clone());
                        }
                    }
                    if let Some(child) = obj.get_mut(name) {
                        apply_defaults_at(root, prop, child, depth + 1);
                    }
                }
            }
            if let Some(values) = node.get("additionalProperties").filter(|v| v.is_object()) {
                for v in obj.values_mut() {
                    apply_defaults_at(root, values, v, depth + 1);
                }
            }
        }
        _ => {}
    }
}
