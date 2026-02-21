#[cfg(test)]
mod tests {

    use gaise_core::contracts::GaiseInstructRequest;
    use serde_json::{from_value, json};

    #[test]
    fn test_add() {

        let jdata = json!({
            "model": "id",
            "input": [
                {
                    "role": "system",
                    "content": {"type": "text", "text": "Cool"}
                },
                {
                    "role": "user",
                    "content": {"type": "text", "text": "my value"}
                },
            ]
        });

        let dx: GaiseInstructRequest = from_value(jdata).unwrap();

        println!("{:?}", dx);
        println!("{:?}", serde_json::to_string_pretty(&dx));
    }

}