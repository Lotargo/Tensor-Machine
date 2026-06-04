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

#[test]
fn test_smart_chunker() {
    use tensor_rag::chunker::SmartChunker;

    let text = "# Chapter XXIII: The Awakening\n\n\
                This is the first sentence of the story. The sun was rising, casting long shadows across the fields.\n\n\
                And this is the second paragraph. It contains another couple of sentences to test splitting!\n\n\
                # Part Two: Escape\n\n\
                This paragraph belongs to the second part. Hope it works.";

    let chunker = SmartChunker::new(150, 30);
    let chunks = chunker.chunk(text);

    for (i, chunk) in chunks.iter().enumerate() {
        println!(
            "Chunk {}: index={}, chapter_id={}, text={:?}",
            i, chunk.chunk_index, chunk.chapter_id, chunk.text
        );
    }

    assert!(!chunks.is_empty());
    // Verify chapter extraction
    assert_eq!(chunks[0].chapter_id, "23");
    // Verify part extraction
    assert_eq!(chunks[chunks.len() - 1].chapter_id, "2");

    // Verify meaningless filtering
    let meaningless_chunks = chunker.chunk(".\n\n");
    assert!(meaningless_chunks.is_empty());
}
