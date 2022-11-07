# strudach

Strudach aims to be a more lightweight alternative to [JSON Schema](https://json-schema.org).

> **STRUC**tured **DA**ta **CH**ecker

## Features

### A schema that looks like the objects it describes

```json
{
  "lorem": "string",
  "dolor": ["number"],
  "sit": {
    "amet": "float"
  }
}
```

### Genericity as toppings, not necessary cruft

```json
{
  "redirects": {
    "(url)": "url"
  },
  "metadata": {
    "added_at": "date",
    "(additional properties)": true
  }
}
```

### Documentation as a fully integrated, first-class feature

```yaml
{ "redirects": {
      "(url, from)": "url, to",
      # what is added after a comma counts as documentation for the key or value
    } }
```

### Most data interchange formats are supported

Both input files and schemas can be written in JSON, YAML, JSON5, TOML, and HOCON.

```sh-session
$ strudach schema.toml data.json5 other-data.yaml
```

### JSON Schemas are supported

You can convert to and from JSON Schemas

```sh-session
$ cat input.strudach.json
```

```json
{
  "redirects": {
    "(url, from)": "url, to"
  },
  "metadata": {
    "added_at": "date",
    "(additional properties)": true
  }
}
```

```sh-session
$ strudach convert input.strudach.json
```

```yaml
{
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "definitions": {
        "url": {
            "type": "string",
            "format": "uri",
            "pattern": "^(https:?|wss?|ftp)://"
        },
    },
    "properties": {
        "redirects": {
            "propertyNames": "url",
            "description": "Maps from to to" # generated from documentation of key and value,
            "$ref": "#/definitions/url",
        },
        "metadata": {
            "type": "object",
            "properties": {
                "added_at": {
                    "type": "string",
                    "format": "date",
                }
            },
            "additionalProperties": true
        }
    }
}
```

## Installation

```sh-session
$ cargo install strudach
```

## Usage 

### In Rust code

```rust
use strudach;

fn main() {
  ...
  match strudach::validate() {
    Ok(_) => ...,
    Err(err) => {
      // You can access err to handle the error programmatically, or:
      println!(err) // strudach errors implement Display, using ariadne.
    }
  }
  ...
}
```

### On the command line

```sh-session
$ strudach schema-file.whatever your-file-to-validate.json other-file.yaml ...
```

stdin will be used for the file to validate by default. Use `strudach - file-to-validate.json` to use stdin for the schema file.
