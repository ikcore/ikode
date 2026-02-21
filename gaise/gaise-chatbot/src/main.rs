use gaise_core::GaiseClient;
use gaise_core::contracts::{GaiseContent, GaiseInstructRequest, GaiseMessage, OneOrMany, GaiseStreamChunk};
use gaise_provider_ollama::ollama_client::GaiseClientOllama;
use futures_util::StreamExt;
use std::io::{self, Write};
use indicatif::{ProgressBar, ProgressStyle};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = GaiseClientOllama::new("http://localhost:11434".to_string());
    let model = "gpt-oss:20b".to_string();


    println!("Welcome to GAISe Chatbot!");
    println!("Using model: {}", model);
    println!("Type 'exit' or 'quit' to stop, 'clear' to reset history.\n");

    let mut history: Vec<GaiseMessage> = Vec::new();

    loop {
        print!("You: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            break;
        }

        if input == "clear" {
            history.clear();
            println!("History cleared.\n");
            continue;
        }

        let user_message = GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: input.to_string(),
            })),
            ..Default::default()
        };

        history.push(user_message);

        let request = GaiseInstructRequest {
            model: model.clone(),
            input: OneOrMany::Many(history.clone()),
            ..Default::default()
        };

        let pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::default_spinner()
            .template("{spinner:.green} Thinking...")
            .unwrap());
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let stream_res = client.instruct_stream(&request).await;

        match stream_res {
            Ok(mut stream) => {
                let mut first_chunk = true;
                let mut full_response = String::new();
                
                print!("AI: ");
                io::stdout().flush()?;

                while let Some(chunk_res) = stream.next().await {
                    if first_chunk {
                        pb.finish_and_clear();
                        first_chunk = false;
                    }

                    match chunk_res {
                        Ok(response) => {
                            if let GaiseStreamChunk::Text(text) = response.chunk {
                                full_response.push_str(&text);
                                let mut display_text = text.as_str();
                                
                                // Basic logic to detect thought tags if the model uses them
                                if display_text.contains("<thought>") {
                                    println!("\n[Thinking...]");
                                    display_text = display_text.split("<thought>").last().unwrap_or("");
                                }
                                
                                if display_text.contains("</thought>") {
                                    println!("\n[Thought end]");
                                    display_text = display_text.split("</thought>").last().unwrap_or("");
                                }

                                if !display_text.is_empty() {
                                    print!("{}", display_text);
                                    io::stdout().flush()?;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("\nError in stream: {}", e);
                            break;
                        }
                    }
                }
                println!("\n");

                if !full_response.is_empty() {
                    history.push(GaiseMessage {
                        role: "assistant".to_string(),
                        content: Some(OneOrMany::One(GaiseContent::Text {
                            text: full_response,
                        })),
                        ..Default::default()
                    });
                }
            }
            Err(e) => {
                pb.finish_and_clear();
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}
