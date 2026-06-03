use tensor_rag::chunker::{Chunker, TokenChunker};

#[test]
fn test_token_chunker() {
    let text = "Hello world. This is a paragraph. And it has sentences. And another paragraph.";
    let chunker = TokenChunker::new(10, 2, "o200k_harmony");
    let chunks = chunker.chunk(text);
    
    for (i, chunk) in chunks.iter().enumerate() {
        println!("Chunk {}: {:?}", i, chunk);
    }

    assert!(!chunks.is_empty());
}
