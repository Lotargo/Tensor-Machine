#!/usr/bin/env python3
"""
Convert RuWordNet SQLite database to a JSON synonym dictionary
compatible with tensor-rag's QUERY_EXPANSION_DICTIONARY_PATHS format.

Output format: JSON object where keys are normalized terms and
values are arrays of synonym terms.
"""
import sqlite3
import json
import os
import re
import sys

DB_PATH = r'C:\Users\etotm\AppData\Local\Programs\Python\Python314\Lib\site-packages\ruwordnet\static\ruwordnet.db'
OUTPUT_PATH = os.path.join(os.path.dirname(__file__), '..', 'tensor-rag', 'dict', 'ruwordnet_synonyms.json')

def normalize_term(term):
    """Lowercase and strip extra whitespace."""
    return re.sub(r'\s+', ' ', term.strip().lower())

def main():
    if not os.path.exists(DB_PATH):
        print(f"ERROR: Database not found at {DB_PATH}")
        sys.exit(1)

    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()

    # Get all synsets with their words (synonym groups)
    # Only synsets with 2+ words are useful as synonym groups
    cursor.execute("""
        SELECT s.id, s.part_of_speech, GROUP_CONCAT(se.name, '|||') as words
        FROM synset s
        JOIN sense se ON se.synset_id = s.id
        GROUP BY s.id
        HAVING COUNT(se.name) >= 2
    """)

    synonym_groups = []
    total_words = set()

    for row in cursor.fetchall():
        synset_id = row[0]
        pos = row[1]
        words_raw = row[2].split('|||')

        # Normalize and deduplicate words within the group
        words = []
        seen = set()
        for w in words_raw:
            normalized = normalize_term(w)
            if normalized and normalized not in seen and len(normalized) >= 2:
                seen.add(normalized)
                words.append(normalized)
                total_words.add(normalized)

        if len(words) >= 2:
            synonym_groups.append(words)

    conn.close()

    # Build dictionary: for each word, store its synonym group
    dictionary = {}
    for group in synonym_groups:
        for word in group:
            if word not in dictionary:
                dictionary[word] = []
            # Add all other words in the group as synonyms
            for syn in group:
                if syn != word and syn not in dictionary[word]:
                    dictionary[word].append(syn)

    # Write output
    os.makedirs(os.path.dirname(OUTPUT_PATH), exist_ok=True)
    with open(OUTPUT_PATH, 'w', encoding='utf-8') as f:
        json.dump(dictionary, f, ensure_ascii=False, indent=2)

    print(f"RuWordNet dictionary generated:")
    print(f"  Synonym groups: {len(synonym_groups)}")
    print(f"  Unique words:   {len(total_words)}")
    print(f"  Dictionary entries: {len(dictionary)}")
    print(f"  Output: {OUTPUT_PATH}")

    # Show some examples
    print("\n--- Examples ---")
    examples = ['мама', 'дом', 'книга', 'мать', 'человек', 'город', 'война', 'говорить']
    for ex in examples:
        if ex in dictionary:
            syns = dictionary[ex][:5]
            print(f"  {ex} -> {syns}")

if __name__ == '__main__':
    main()
