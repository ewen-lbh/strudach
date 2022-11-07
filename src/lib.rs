use regex::Regex;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    hash::Hash,
    io::BufReader,
    path::{Path, PathBuf},
};

pub struct ValidationError {
    pub file: PathBuf,
    /// Access path to element
    pub path: Vec<String>,
    pub message: String,
}

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
    /// Object(values, additional properties allowed ?)
    Object(HashMap<String, CommentedType>, bool),
    Array(Box<CommentedType>),
    FixedSizeArray(Vec<Box<CommentedType>>),
    OneOf(Vec<CommentedType>),
    AllOf(Vec<CommentedType>),
    RegexPattern(Regex),
    Literal(serde_json::Value),
    /// Written {"(one of literally)": ["a", "b"]} shortcut for {"(one of)": ["literally a", "literally b", …]}
    Enum(Vec<serde_json::Value>),
}

pub type CommentedType = (Type, String);

pub struct Schema {
    pub types: HashMap<String, Type>,
    pub value: CommentedType,
}

fn into_serde_value(path: PathBuf) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match path.extension() {
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
    custom_types: HashMap<String, Type>,
) -> Result<CommentedType, Box<dyn std::error::Error>> {
    let mut documentation = "";
    let value = match value {
        Value::Array(elements) => match elements.len() {
            0 => Type::Literal(value),
            1 => Type::Array(Box::new(load_type(elements[0], custom_types)?)),
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
                    documentation = doc;
                    typestr
                }
                None => s.as_str(),
            };
            match typestring {
                "string" => Type::String,
                "number" => Type::Number,
                "float" => Type::Float,
                "integer" => Type::Integer,
                "boolean" => Type::Boolean,
                "array" => Type::AnyArray,
                "object" => Type::AnyObject,
                "any" => Type::Any,
                "null" => Type::Literal(Value::Null),
                _ if s.starts_with("literally ") => Type::Literal(Value::String(
                    s.strip_prefix("literally ").unwrap().to_string(),
                )),
                _ if s.starts_with("just ") => {
                    Type::Literal(Value::String(s.strip_prefix("just ").unwrap().to_string()))
                }
                _ if s.starts_with("matches regex ") => {
                    Type::RegexPattern(Regex::new(s.strip_prefix("matches regex ").unwrap())?)
                }
                _ => return Err("Invalid type".into()),
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                Type::Literal(value)
            } else {
                match map.keys().next().unwrap().as_str() {
                    "(one of)" | "(all of)" if map.len() == 1 => {
                        let mut types = Vec::new();
                        for value in map.values() {
                            types.push(load_type(value.clone(), custom_types)?);
                        }
                        if map.keys().next().unwrap() == "(one of)" {
                            Type::OneOf(types)
                        } else {
                            Type::AllOf(types)
                        }
                    }
                    "(one of literally)" if map.len() == 1 => {
                        let mut literals = Vec::new();
                        for value in map.values() {
                            literals.push(value.clone());
                        }
                        Type::Enum(literals)
                    }
                    _ => {
                        let mut properties = HashMap::new();
                        let mut additional_properties = false;
                        for (key, value) in map {
                            match key.as_str() {
                                "(additional properties)" => {
                                    additional_properties = value.as_bool().unwrap();
                                }
                                "(types)" => {
                                    let Some(typeshed) = value.as_object() else {
                                        return Err("Typeshed must be an object mapping type names to types".into());
                                    };
                                    for (key, value) in typeshed {
                                        custom_types.insert(
                                            key.clone(),
                                            load_type(value.clone(), custom_types)?.0,
                                        );
                                    }
                                }
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
    let custom_types: HashMap<String, Type> = HashMap::new();
    let (value, documentation) = load_type(value, custom_types)?;
    Ok(Schema {
        types: custom_types,
        value: (value, documentation),
    })
}

pub fn validate_value(
    file: PathBuf,
    location: Vec<String>,
    typ: CommentedType,
    value: Value,
) -> Result<Vec<ValidationError>, Box<dyn std::error::Error>> {
    let mut validation_errors = Vec::new();
    match (value, typ.0) {
        (_, Type::Any) => Ok(Vec::new()),
        (Value::Array(elements), Type::AnyArray) => Ok(Vec::new()),
        (Value::Array(elements), Type::FixedSizeArray(types)) => {
            if elements.len() != types.len() {
                validation_errors.push(ValidationError {
                    message: "Array length does not match fixed size".to_owned(),
                    path: location.clone(),
                    file,
                });
            } else {
                for (i, element) in elements.iter().enumerate() {
                    validation_errors.append(&mut validate_value(
                        file,
                        {
                            let newloc = location.clone();
                            newloc.push(i.to_string());
                            newloc
                        },
                        *types[i].clone(),
                        element.clone(),
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
        (Value::String(_), Type::String) => Ok(Vec::new()),
        (Value::String(s), Type::RegexPattern(regex)) => {
            if !regex.is_match(&s) {
                validation_errors.push(ValidationError {
                    message: "String does not match regex".to_owned(),
                    path: location.clone(),
                    file,
                });
            }
            Ok(validation_errors)
        }
        (Value::String(s), Type::Literal(Value::String(literal))) => {
            if s != literal {
                validation_errors.push(ValidationError {
                    message: "String is not literally".to_owned(),
                    path: location.clone(),
                    file,
                });
            }
            Ok(validation_errors)
        }
        (Value::Object(map), Type::Object(properties, additional_properties)) => {
            for (key, value) in map {
                if properties.contains_key(&key) {
                    validation_errors.append(&mut validate_value(
                        file,
                        {
                            let mut newloc = location.clone();
                            newloc.push(key);
                            newloc
                        },
                        properties[&key].clone(),
                        value,
                    )?);
                } else if !additional_properties {
                    validation_errors.push(ValidationError {
                        message: "Object has additional properties".to_owned(),
                        path: location.clone(),
                        file,
                    });
                }
            }
            Ok(validation_errors)
        }
        (value, Type::AllOf(types)) => {
            for typ in types {
                validation_errors.append(&mut validate_value(
                    file,
                    location.clone(),
                    typ,
                    value.clone(),
                )?);
            }
            Ok(validation_errors)
        }
        (value, Type::OneOf(types)) => {
            let mut valid = false;
            for typ in types {
                let mut errors = validate_value(file, location.clone(), typ, value.clone())?;
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
                    message: format!("Value is not any of the allowed values: {:?}", literals)
                        .to_owned(),
                    path: location.clone(),
                    file,
                });
            }
            Ok(validation_errors)
        },
        _ => {
            validation_errors.push(ValidationError {
                message: "Value does not match type".to_owned(),
                path: location.clone(),
                file,
            });
            Ok(validation_errors)
        }
    }
}

pub fn validate_one(
    schema: &Schema,
    input_file: PathBuf,
) -> Result<Vec<ValidationError>, Box<dyn std::error::Error>> {
    let validation_errors = validate_value(
        input_file,
        Vec::new(),
        schema.value,
        serde_json::from_reader(File::open(input_file)?)?,
    )?;
    Ok(validation_errors)
}

pub fn validate(
    schema: Schema,
    input_files: Vec<PathBuf>,
) -> Result<Vec<ValidationError>, Box<dyn std::error::Error>> {
    let validation_errors = vec![];
    for file in input_files {
        validation_errors.extend(validate_one(&schema, file)?);
    }
    Ok(validation_errors)
}
