use serde::Serialize;

fn write_value(value: &serde_json::Value, output: &mut String) -> Result<(), serde_json::Error> {
    match value {
        serde_json::Value::Null => output.push_str("null"),
        serde_json::Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        serde_json::Value::Number(value) => output.push_str(&value.to_string()),
        serde_json::Value::String(value) => output.push_str(&serde_json::to_string(value)?),
        serde_json::Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_value(value, output)?;
            }
            output.push(']');
        }
        serde_json::Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            output.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key)?);
                output.push(':');
                write_value(&values[key], output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

pub(crate) fn to_canonical_json<T: Serialize + ?Sized>(
    value: &T,
) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(value)?;
    let mut output = String::new();
    write_value(&value, &mut output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::to_canonical_json;

    #[test]
    fn recursively_sorts_object_keys_without_reordering_arrays() {
        let mut nested = HashMap::new();
        nested.insert("zeta", 2);
        nested.insert("alpha", 1);
        let value = vec![nested.clone(), nested];
        let json = to_canonical_json(&value).unwrap();
        assert_eq!(
            json,
            "[{\"alpha\":1,\"zeta\":2},{\"alpha\":1,\"zeta\":2}]"
        );
    }
}
