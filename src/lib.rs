use regex::Regex;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
};

pub struct ValidationError {
    pub file: PathBuf,
    /// Access path to element
    pub path: Vec<String>,
    pub message: String,
}

pub type Typeshed = HashMap<String, CommentedType>;

#[derive(Debug, Clone)]
pub enum Type {
    String,
    Number,
    Float,
    Integer,
    Boolean,
    AnyArray,
    AnyObject,
    Any,
    Color,
    Date,
    DateTime,
    Time,
    HTML,
    URL,
    /// Object(values, additional properties allowed ?)
    Object(HashMap<String, CommentedType>, bool),
    Array(Box<CommentedType>),
    FixedSizeArray(Vec<Box<CommentedType>>),
    OneOf(Vec<CommentedType>),
    AllOf(Vec<CommentedType>),
    RegexPattern(Regex),
    Literal(serde_json::Value),
    LiteralString(String),
    /// Written {"(one of literally)": ["a", "b"]} shortcut for {"(one of)": ["literally a", "literally b", …]}
    Enum(Vec<serde_json::Value>),
    Custom(String, Box<CommentedType>),
}

fn warn(txt: &'static str) -> () {
    println!("WARN: {}", txt)
}

impl core::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.type_name())
    }
}

impl Type {
    fn type_name(&self) -> String {
        match self {
            Type::String => "string".to_owned(),
            Type::Number => "number".to_owned(),
            Type::Float => "float".to_owned(),
            Type::Integer => "integer".to_owned(),
            Type::Color => "color".to_owned(),
            Type::Boolean => "boolean".to_owned(),
            Type::AnyArray => "array".to_owned(),
            Type::AnyObject => "object".to_owned(),
            Type::Any => "any".to_owned(),
            Type::Date => "date".to_owned(),
            Type::DateTime => "datetime".to_owned(),
            Type::Time => "time".to_owned(),
            Type::HTML => "html".to_owned(),
            Type::URL => "url".to_owned(),
            Type::Object(_, _) => "object".to_owned(),
            Type::Array(_) => "array".to_owned(),
            Type::FixedSizeArray(_) => "array".to_owned(),
            Type::OneOf(types) => format!(
                "one of {}",
                types
                    .iter()
                    .map(|t| t.0.type_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::AllOf(types) => format!(
                "all of {}",
                types
                    .iter()
                    .map(|t| t.0.type_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::RegexPattern(_) => "regex pattern".to_owned(),
            Type::Literal(value) => serde_type_name(value),
            Type::LiteralString(_) => "string".to_owned(),
            Type::Enum(_) => "enum".to_owned(),
            Type::Custom(name, _) => name.clone(),
        }
    }
}

pub type CommentedType = (Type, String);

pub struct Schema {
    pub types: Typeshed,
    pub value: CommentedType,
}

fn into_serde_value(path: PathBuf) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match path.extension().map(|s| s.to_str().unwrap_or_default()) {
        Some("yaml") | Some("yml") => {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            let value: serde_json::Value = serde_yaml::from_reader(reader)?;
            Ok(value)
        }
        Some("toml") => {
            let content = fs::read_to_string(path)?;
            let value: serde_json::Value = toml::from_str(&content)?;
            Ok(value)
        }
        _ => {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            let value: serde_json::Value = serde_json::from_reader(reader)?;
            Ok(value)
        }
    }
}

fn load_type(
    value: serde_json::Value,
    custom_types: &mut Typeshed,
) -> Result<CommentedType, Box<dyn std::error::Error>> {
    let mut documentation = "".to_owned();
    let value = match value.clone() {
        Value::Array(elements) => match elements.len() {
            0 => Type::Literal(value),
            1 => Type::Array(Box::new(load_type(elements[0].clone(), custom_types)?)),
            _ => {
                let mut types = Vec::new();
                for element in elements {
                    types.push(Box::new(load_type(element, custom_types)?));
                }
                Type::FixedSizeArray(types)
            }
        },
        Value::Bool(_) | Value::Null | Value::Number(_) => Type::Literal(value),
        Value::String(s) => {
            let typestring = match s.split_once(", ") {
                Some((typestr, doc)) => {
                    documentation = doc.clone().to_string();
                    typestr.to_owned()
                }
                None => s.to_string(),
            };
            match typestring.as_str() {
                "string" => Type::String,
                "number" => Type::Number,
                "float" => Type::Float,
                "integer" => Type::Integer,
                "boolean" => Type::Boolean,
                "array" => Type::AnyArray,
                "object" => Type::AnyObject,
                "date" => Type::Date,
                "datetime" => Type::DateTime,
                "empty string" => Type::LiteralString("".to_owned()),
                "time" => Type::Time,
                "color" => Type::Color,
                "any" => Type::Any,
                "url" => Type::URL,
                "html" => Type::HTML,
                "null" => Type::Literal(Value::Null),
                _ if typestring.starts_with("literally ") => {
                    Type::LiteralString(typestring["literally ".len()..].to_string())
                }
                _ if typestring.starts_with("just ") => {
                    Type::LiteralString(typestring["just ".len()..].to_string())
                }
                _ if typestring.starts_with("'") && typestring.ends_with("'") -> {
                    Type::LiteralString(typestring[1..typestring.len()-1].to_string())
                }
                _ if typestring.starts_with("matches regex ") => Type::RegexPattern(Regex::new(
                    typestring.strip_prefix("matches regex ").unwrap(),
                )?),
                _ if custom_types.contains_key(&typestring) => Type::Custom(
                    typestring.to_string(),
                    Box::new(custom_types[&typestring].clone()),
                ),
                _ => return Err(format!("Invalid type {:?}", s).into()),
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                Type::Literal(value.clone())
            } else {
                match map.keys().next().unwrap().as_str() {
                    "(one of)" | "(all of)" if map.len() == 1 => {
                        let mut types = Vec::new();
                        if let Some(Value::Array(specs)) = map.values().next() {
                            for value in specs {
                                types.push(load_type(value.clone(), custom_types)?);
                            }
                        } else {
                            return Err(format!(
                                "{} must be an array of possibles types",
                                map.keys().next().unwrap()
                            )
                            .into());
                        }
                        if map.keys().next().unwrap() == "(one of)" {
                            Type::OneOf(types)
                        } else {
                            Type::AllOf(types)
                        }
                    }
                    "(one of literally)" | "(enum)" if map.len() == 1 => {
                        let mut literals = Vec::new();
                        for value in map.values() {
                            literals.push(value.clone());
                        }
                        Type::Enum(literals)
                    }
                    _ => {
                        let mut properties = HashMap::new();
                        let mut additional_properties = false;
                        if map.contains_key("(types)") {
                            let value = map["(types)"].clone();
                            let Some(typeshed) = value.as_object() else {
                                return Err("Typeshed must be an object mapping type names to types".into());
                            };
                            for (key, value) in typeshed {
                                let loaded_type = load_type(value.clone(), custom_types)?;
                                custom_types.insert(key.clone(), loaded_type);
                            }
                        }
                        for (key, value) in map {
                            match key.as_str() {
                                "(additional properties)" | "(additional keys)" => {
                                    additional_properties = value.as_bool().unwrap();
                                }
                                "(types)" => {}
                                _ => {
                                    properties.insert(key, load_type(value, custom_types)?);
                                }
                            }
                        }
                        Type::Object(properties, additional_properties)
                    }
                }
            }
        }
    };
    Ok((value, documentation.to_string()))
}

pub fn load(path: PathBuf) -> Result<Schema, Box<dyn std::error::Error>> {
    let value = into_serde_value(path)?;
    let mut custom_types: HashMap<String, CommentedType> = HashMap::new();
    let (typ, documentation) = load_type(value, &mut custom_types)?;
    Ok(Schema {
        types: custom_types,
        value: (typ, documentation),
    })
}

fn serde_type_name(object: &Value) -> String {
    match object {
        Value::Array(_) => "array".to_string(),
        Value::Bool(_) => "boolean".to_string(),
        Value::Null => "null".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Object(_) => "object".to_string(),
    }
}

pub fn validate_value(
    file: PathBuf,
    location: Vec<String>,
    typ: &CommentedType,
    value: &Value,
    custom_types: &mut Typeshed,
) -> Result<Vec<ValidationError>, Box<dyn std::error::Error>> {
    println!(
        "at {}:{}: looking for {} in {} value",
        file.display(),
        location.join("."),
        typ.0,
        serde_type_name(value)
    );
    let mut validation_errors = Vec::new();
    match (value, &typ.0) {
        (_, Type::Any) => Ok(Vec::new()),
        (Value::Array(_), Type::AnyArray) => Ok(Vec::new()),
        (Value::Array(elements), Type::Array(elements_type)) => {
            for (i, element) in elements.iter().enumerate() {
                validation_errors.append(&mut validate_value(
                    file.clone(),
                    {
                        let mut newloc = location.clone();
                        newloc.push(i.to_string());
                        newloc
                    },
                    &elements_type,
                    element,
                    custom_types,
                )?);
            }
            Ok(validation_errors)
        }
        (Value::Array(elements), Type::FixedSizeArray(types)) => {
            if elements.len() != types.len() {
                validation_errors.push(ValidationError {
                    message: format!("Array length does not match fixed size {}", types.len())
                        .to_owned(),
                    path: location.clone(),
                    file: file.clone(),
                });
            } else {
                for (i, element) in elements.iter().enumerate() {
                    validation_errors.append(&mut validate_value(
                        file.clone(),
                        {
                            let mut newloc = location.clone();
                            newloc.push(i.to_string());
                            newloc
                        },
                        &types[i],
                        element,
                        custom_types,
                    )?);
                }
            }
            Ok(validation_errors)
        }
        (Value::Bool(_), Type::Boolean) => Ok(Vec::new()),
        (Value::Null, Type::Literal(Value::Null)) => Ok(Vec::new()),
        (Value::Number(_), Type::Number) => Ok(Vec::new()),
        (Value::Number(_), Type::Float) => {
            if value.as_f64().unwrap().fract() == 0.0 {
                validation_errors.push(ValidationError {
                    message: "Number is not a float".to_owned(),
                    path: location.clone(),
                    file,
                });
            }
            Ok(validation_errors)
        }
        (Value::Number(_), Type::Integer) => {
            if value.as_f64().unwrap().fract() != 0.0 {
                validation_errors.push(ValidationError {
                    message: "Number is not an integer".to_owned(),
                    path: location.clone(),
                    file,
                });
            }
            Ok(validation_errors)
        }
        (Value::String(s), Type::Color) => match s.parse::<css_color::Srgb>() {
            Ok(_) => Ok(Vec::new()),
            Err(e) => {
                validation_errors.push(ValidationError {
                    message: format!("String is not a valid color: {:?}", e),
                    path: location.clone(),
                    file,
                });
                Ok(validation_errors)
            }
        },
        (Value::String(s), Type::HTML) => match html_parser::Dom::parse(s) {
            Ok(_) => Ok(Vec::new()),
            Err(html_parser::Error::Parsing(e)) => {
                validation_errors.push(ValidationError {
                    message: format!("String is not valid HTML: {}", e),
                    path: location.clone(),
                    file,
                });
                Ok(validation_errors)
            }
            Err(e) => {
                return Err(format!("Error while validating HTML: {:?}", e).into());
            }
        },
        (Value::String(s), Type::Date) => match iso8601::date(s) {
            Ok(_) => Ok(Vec::new()),
            Err(e) => {
                validation_errors.push(ValidationError {
                    message: format!("String is not a valid date: {}", e),
                    path: location.clone(),
                    file,
                });
                Ok(validation_errors)
            }
        },
        (Value::String(s), Type::DateTime) => match iso8601::datetime(s) {
            Ok(_) => Ok(Vec::new()),
            Err(e) => {
                validation_errors.push(ValidationError {
                    message: format!("String is not a valid datetime: {}", e),
                    path: location.clone(),
                    file,
                });
                Ok(validation_errors)
            }
        },
        (Value::String(s), Type::Time) => match iso8601::time(s) {
            Ok(_) => Ok(Vec::new()),
            Err(e) => {
                validation_errors.push(ValidationError {
                    message: format!("String is not a valid time: {}", e),
                    path: location.clone(),
                    file,
                });
                Ok(validation_errors)
            }
        },
        (Value::String(s), Type::URL) => {
            if validator::validate_url(s) {
                Ok(validation_errors)
            } else {
                validation_errors.push(ValidationError {
                    message: "String is not a valid URL".to_owned(),
                    path: location.clone(),
                    file,
                });
                Ok(validation_errors)
            }
        }
        (Value::String(_), Type::String) => Ok(Vec::new()),
        (Value::String(s), Type::RegexPattern(regex)) => {
            if !regex.is_match(&s) {
                validation_errors.push(ValidationError {
                    message: format!("String does not match regex {}", regex).to_owned(),
                    path: location.clone(),
                    file,
                });
            }
            Ok(validation_errors)
        }
        (
            Value::String(s),
            Type::Literal(Value::String(ref literal)) | Type::LiteralString(ref literal),
        ) => {
            if s != literal {
                validation_errors.push(ValidationError {
                    message: format!("String is not literally {:?}", literal).to_owned(),
                    path: location.clone(),
                    file: file.clone(),
                });
            }
            Ok(validation_errors)
        }
        (Value::Object(map), Type::Object(properties, additional_properties)) => {
            for (key, value) in map {
                if properties.contains_key(key) {
                    validation_errors.append(&mut validate_value(
                        file.clone(),
                        {
                            let mut newloc = location.clone();
                            newloc.push(key.to_string());
                            newloc
                        },
                        &properties[key],
                        value,
                        custom_types,
                    )?);
                    // If there's a generic key in the type's properties, we check that additional keys conform.
                } else if let Some(generic_key) = properties
                    .keys()
                    .find(|k| k.starts_with("(") && k.ends_with(")"))
                    .map(|k| k.strip_prefix("(").unwrap().strip_suffix(")").unwrap())
                {
                    let key_type = load_type(Value::String(generic_key.to_owned()), custom_types)?;
                    validation_errors.append(&mut validate_value(
                        file.clone(),
                        {
                            let mut newloc = location.clone();
                            newloc.push("(key)".to_string());
                            newloc
                        },
                        &key_type,
                        &Value::String(key.to_string()),
                        custom_types,
                    )?);
                    validation_errors.append(&mut validate_value(
                        file.clone(),
                        {
                            let mut newloc = location.clone();
                            newloc.push(key.to_string());
                            newloc
                        },
                        &properties[&format!("({})", generic_key)],
                        value,
                        custom_types,
                    )?);
                } else if !additional_properties {
                    validation_errors.push(ValidationError {
                        message: format!("Object has additional property `{}`", key).to_owned(),
                        path: location.clone(),
                        file: file.clone(),
                    });
                }
            }
            let missing_keys = properties
                .keys()
                .filter(|key| !(key.starts_with("(") && key.ends_with(")")))
                .filter(|key| !map.contains_key(*key))
                .collect::<Vec<_>>();
            if !missing_keys.is_empty() {
                validation_errors.push(ValidationError {
                    message: format!(
                        "Object is missing {} {}",
                        if missing_keys.len() == 1 {
                            "property"
                        } else {
                            "properties"
                        },
                        missing_keys
                            .into_iter()
                            .map(|k| format!("`{}`", k))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .to_owned(),
                    path: location.clone(),
                    file: file.clone(),
                });
            }
            Ok(validation_errors)
        }
        (value, Type::AllOf(types)) => {
            for typ in types {
                validation_errors.append(&mut validate_value(
                    file.clone(),
                    location.clone(),
                    &typ,
                    value,
                    custom_types,
                )?);
            }
            Ok(validation_errors)
        }
        (value, Type::OneOf(types)) => {
            let mut valid = false;
            for typ in types {
                let mut errors =
                    validate_value(file.clone(), location.clone(), &typ, value, custom_types)?;
                if errors.is_empty() {
                    valid = true;
                    break;
                } else {
                    validation_errors.append(&mut errors);
                }
            }
            if !valid {
                validation_errors.push(ValidationError {
                    message: "Value does not match any of the types".to_owned(),
                    path: location.clone(),
                    file,
                });
            }
            Ok(validation_errors)
        }
        (value, Type::Enum(literals)) => {
            let mut valid = false;
            for literal in literals {
                if value == literal {
                    valid = true;
                    break;
                }
            }
            if !valid {
                validation_errors.push(ValidationError {
                    message: format!(
                        "Value is not any of the allowed values: {}",
                        literals
                            .into_iter()
                            .map(|l| format!("{:?}", l))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .to_owned(),
                    path: location.clone(),
                    file,
                });
            }
            Ok(validation_errors)
        }
        (value, Type::Custom(type_name, spec)) => {
            println!(
                "Validating value of custom type {} with spec {:#?}",
                type_name, spec
            );
            let validation_sub_errors =
                validate_value(file.clone(), location.clone(), spec, value, custom_types)?;
            if !validation_sub_errors.is_empty() {
                validation_errors.append(
                    &mut validation_sub_errors
                        .into_iter()
                        .map(|e| ValidationError {
                            message: format!("Custom type `{}`: {}", type_name, e.message),
                            path: e.path,
                            file: e.file,
                        })
                        .collect::<Vec<_>>(),
                );
            }
            Ok(validation_errors)
        }
        _ => {
            validation_errors.push(ValidationError {
                message: format!(
                    "Value has type {}, which does not match type {}",
                    serde_type_name(value),
                    typ.0.type_name()
                )
                .to_owned(),
                path: location.clone(),
                file,
            });
            Ok(validation_errors)
        }
    }
}

pub fn validate_one(
    schema: &mut Schema,
    input_file: PathBuf,
) -> Result<Vec<ValidationError>, Box<dyn std::error::Error>> {
    let validation_errors = validate_value(
        input_file.clone(),
        Vec::new(),
        &schema.value,
        &serde_json::from_reader(File::open(input_file)?)?,
        &mut schema.types,
    )?;
    Ok(validation_errors)
}

pub fn validate(
    schema: &mut Schema,
    input_files: Vec<PathBuf>,
) -> Result<Vec<ValidationError>, Box<dyn std::error::Error>> {
    let mut validation_errors = vec![];
    for file in input_files {
        validation_errors.extend(validate_one(schema, file)?);
    }
    Ok(validation_errors)
}

pub fn to_jsonschema(schema: &Schema) -> serde_json::Value {
    let mut jsonschema = type_to_jsonschema(&schema.value);
    let mut definitions = serde_json::Map::new();
    for (name, typ) in schema.types.iter() {
        definitions.insert(
            name.clone(),
            serde_json::Value::Object(type_to_jsonschema(typ)),
        );
    }
    jsonschema.insert("$defs".to_string(), serde_json::Value::Object(definitions));
    return serde_json::Value::Object(jsonschema);
}

pub fn type_to_jsonschema(value: &CommentedType) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();

    if value.1 != "" {
        out.insert("description".to_string(), Value::String(value.1.clone()));
    }

    match &value.0 {
        Type::AllOf(types) => {
            out.insert(
                "allOf".to_string(),
                serde_json::Value::Array(
                    types
                        .into_iter()
                        .map(|t| serde_json::Value::Object(type_to_jsonschema(&t)))
                        .collect(),
                ),
            );
        }
        Type::Any => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("any".to_string()),
            );
        }
        Type::AnyArray => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("array".to_string()),
            );
        }
        Type::AnyObject => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("object".to_string()),
            );
        }
        Type::Array(typ) => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("array".to_string()),
            );
            out.insert(
                "items".to_string(),
                serde_json::Value::Object(type_to_jsonschema(&typ)),
            );
        }
        Type::Boolean => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("boolean".to_string()),
            );
        }
        Type::Color => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            warn("Color type is not supported in JSON Schema, yet.");
        }
        Type::Custom(typename, _) => {
            out.insert(
                "$ref".to_string(),
                serde_json::Value::String(format!("#/definitions/{}", typename)),
            );
        }
        Type::Date => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            out.insert(
                "format".to_string(),
                serde_json::Value::String("date".to_string()),
            );
        }
        Type::DateTime => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            out.insert(
                "format".to_string(),
                serde_json::Value::String("date-time".to_string()),
            );
        }
        Type::Enum(literals) => {
            out.insert(
                "enum".to_string(),
                serde_json::Value::Array(
                    literals
                        .into_iter()
                        .map(|l| serde_json::Value::String(l.to_string()))
                        .collect(),
                ),
            );
        }
        Type::FixedSizeArray(types) => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("array".to_string()),
            );
            out.insert(
                "items".to_string(),
                serde_json::Value::Array(
                    types
                        .into_iter()
                        .map(|t| serde_json::Value::Object(type_to_jsonschema(&t)))
                        .collect(),
                ),
            );
        }
        Type::Float => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("number".to_string()),
            );
            out.insert(
                "format".to_string(),
                serde_json::Value::String("float".to_string()),
            );
        }
        Type::HTML => {
            warn("HTML is not convertible to json schema");
            out.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
        }
        Type::Integer => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("integer".to_string()),
            );
        }
        Type::Literal(lit) => {
            out.insert("const".to_string(), lit.clone());
        }
        Type::LiteralString(s) => {
            out.insert("const".to_string(), Value::String(s.to_string()));
        }
        Type::Number => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("number".to_string()),
            );
        }
        Type::Object(obj, additional_props) => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("object".to_string()),
            );
            let mut props = serde_json::Map::new();
            for (key, value) in obj {
                props.insert(
                    key.to_string(),
                    serde_json::Value::Object(type_to_jsonschema(value)),
                );
            }
            out.insert("properties".to_string(), serde_json::Value::Object(props));
            if *additional_props {
                out.insert(
                    "additionalProperties".to_string(),
                    serde_json::Value::Bool(true),
                );
            }
        }
        Type::OneOf(types) => {
            out.insert(
                "oneOf".to_string(),
                serde_json::Value::Array(
                    types
                        .into_iter()
                        .map(|t| serde_json::Value::Object(type_to_jsonschema(&t)))
                        .collect(),
                ),
            );
        }
        Type::RegexPattern(pat) => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            out.insert(
                "pattern".to_string(),
                serde_json::Value::String(pat.to_string()),
            );
        }
        Type::String => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
        }
        Type::Time => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            out.insert(
                "format".to_string(),
                serde_json::Value::String("time".to_string()),
            );
        }
        Type::URL => {
            out.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            out.insert(
                "format".to_string(),
                serde_json::Value::String("uri".to_string()),
            );
        }
    }

    return out;
}
