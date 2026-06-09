import sqlite3
import os

db_path = r'C:\Users\etotm\AppData\Local\Programs\Python\Python314\Lib\site-packages\ruwordnet\static\ruwordnet.db'
if not os.path.exists(db_path):
    print('Database not found')
    exit(1)

size_mb = os.path.getsize(db_path) / (1024*1024)
print(f'Database size: {size_mb:.1f} MB')

conn = sqlite3.connect(db_path)
cursor = conn.cursor()

cursor.execute("SELECT name FROM sqlite_master WHERE type='table'")
tables = cursor.fetchall()
print(f'Tables: {[t[0] for t in tables]}')

for table in tables:
    table_name = table[0]
    cursor.execute(f'PRAGMA table_info({table_name})')
    columns = cursor.fetchall()
    print(f'\n{table_name}:')
    for col in columns:
        print(f'  {col[1]} ({col[2]})')
    cursor.execute(f'SELECT COUNT(*) FROM {table_name}')
    count = cursor.fetchone()[0]
    print(f'  Rows: {count}')

print('\n--- Sample synset ---')
cursor.execute("SELECT * FROM synset LIMIT 5")
for row in cursor.fetchall():
    print(row)

print('\n--- Sample sense ---')
cursor.execute("SELECT * FROM sense LIMIT 10")
for row in cursor.fetchall():
    print(row)

print('\n--- Synonym groups (words per synset) ---')
cursor.execute("""
    SELECT s.id, s.title, s.part_of_speech, GROUP_CONCAT(se.name, ', ') as words
    FROM synset s
    JOIN sense se ON se.synset_id = s.id
    GROUP BY s.id
    HAVING COUNT(se.name) >= 2
    LIMIT 20
""")
for row in cursor.fetchall():
    print(row)

conn.close()
