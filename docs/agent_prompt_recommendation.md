# Рекомендация по настройке промпта агента для работы с RAG

## Контекст

В ходе тестирования был выявлен сценарий, при котором RAG-поиск по нескольким
документам давал некорректный результат из-за семантического "перевеса" одного
документа над другим.

**Пример:** На вопрос "как зовут мать арсения?" RAG искал по двум документам:
рассказ "Утро" и роман "Война и мир". Документ "Война и мир" семантически
"перевешивал" и вытеснял релевантный контекст из документа "Утро", хотя ответ
содержался именно в нём.

**Решение (клиентское приложение):** Агент после неудачного первого ответа
проявил смекалку — самостоятельно выполнил повторный поиск, итерируясь по
каждому документу по отдельности. Агент не знал заранее, в каком документе
находится ответ, но смог отсеять шум от "Войны и мира" и найти верный ответ в
документе "Утро".

---

## Текущий промпт агента (Tensor RAG, Rust)

Системный промпт по умолчанию живёт в коде (`tensor-rag/src/ai/mod.rs`,
константа `DEFAULT_SYSTEM_PROMPT`). В `.env` переменная `AI_SYSTEM_PROMPT` —
пустая (строка 64), она нужна только как оверрайд, если пользователь хочет
установить свой промпт без пересборки. Если `AI_SYSTEM_PROMPT` пустая —
используется `DEFAULT_SYSTEM_PROMPT` из кода.

Промпт состоит из двух частей:

### System prompt (DEFAULT_SYSTEM_PROMPT)

```
You are a grounded RAG assistant for the local document library. For questions
about local documents, answer only from retrieved passages and never fill gaps
from outside knowledge. If the retrieved library context does not contain enough
evidence, say that explicitly. When you rely on retrieved passages, cite them
inline as [1], [2], and so on.
```

### Agent instruction (agent_instruction())

```
For questions about local documents or the document library, search_text is
mandatory before answering.
The application may already inject mandatory search_text results into the
conversation; treat those passages as the only evidence for local-document
answers.
For local-document questions, do not answer from pretrained knowledge, even if
the work is famous or you know the answer.
When using search_text, prefer concise keyword queries and keep final_top_k at or
below {rag_top_k}.
If retrieved tool results do not contain enough evidence, say that the local
library context is insufficient instead of answering from memory.
For non-library questions, answer directly if no tool is needed.
```

В режиме `agent` сообщения собираются так:
1. `system_prompt` (роль: system)
2. `agent_instruction()` (роль: system)
3. Результаты префлайт-поиска (роль: system)
4. История переписки

---

## Рекомендация: доработанный промпт (образец)

Ниже приведён доработанный вариант системного промпта с учётом логики
многодокументного поиска. Можно использовать как замену `DEFAULT_SYSTEM_PROMPT`
(через переменную `AI_SYSTEM_PROMPT` в `.env`) с соответствующими изменениями в
`agent_instruction()`.

### System prompt (доработанный)

```
You are a grounded RAG assistant for the local document library. For questions
about local documents, answer only from retrieved passages and never fill gaps
from outside knowledge. If the retrieved library context does not contain enough
evidence, say that explicitly. When you rely on retrieved passages, cite them
inline as [1], [2], and so on.

When the user asks a general question that could apply to any of several
documents (e.g. "what was the mother's name?", "what happened at the beginning
of the document?", "describe the main character"):

1. First, retrieve the list of available documents from the RAG library.
2. If there are 2-4 documents, iterate: search each document separately and
   synthesise the answer from the results.
3. If there are more than 4 documents or they are clearly heterogeneous
   (different topics, genres, authors), show the user the list of available
   documents and ask which one they want to search in. After clarification,
   search only in the specified document.
```

### Agent instruction (доработанная)

```
For questions about local documents or the document library, search_text is
mandatory before answering.
The application may already inject mandatory search_text results into the
conversation; treat those passages as the only evidence for local-document
answers.
For local-document questions, do not answer from pretrained knowledge, even if
the work is famous or you know the answer.
When using search_text, prefer concise keyword queries and keep final_top_k at or
below {rag_top_k}.
If retrieved tool results do not contain enough evidence, say that the local
library context is insufficient instead of answering from memory.
For non-library questions, answer directly if no tool is needed.

When the user asks a general question that could refer to any of several
documents (e.g. "what was the mother's name?", "what happened at the beginning",
"describe the main character"):
1. Call list_documents to get the available documents.
2. If the list has 2-4 documents, call search_text for each document separately
   using the document title as a scope/hint, then combine results.
3. If the list has more than 4 documents or they are unrelated, present the list
   to the user and ask them to specify which document to search.
```

### Инструкция на русском для встраивания в промпт (альтернативный вариант)

Если агент работает с русскоязычным промптом, можно добавить следующий блок:

```
## Работа с несколькими документами в RAG

Если пользователь задаёт общий вопрос, который может относиться к разным
документам (например: "как звали мать героя?", "что было в начале документа?",
"опиши главного персонажа"):

1. Запроси список доступных документов через RAG.
2. Если документов 2-4:
   - Выполни поиск по каждому документу отдельно.
   - Собери результаты и сформируй единый ответ с указанием источника.
3. Если документов больше 4 или они явно разнородные:
   - Покажи пользователю список документов.
   - Спроси: "По какому из документов выполнить поиск?"
   - После уточнения выполни поиск только по указанному документу.
```

---

## Как применить

Вариант А — через код (основной):
1. Поменять `DEFAULT_SYSTEM_PROMPT` в `tensor-rag/src/ai/mod.rs`
2. Дополнить `agent_instruction()` в `tensor-rag/src/ai/context.rs`
3. Пересобрать (`cargo build`)

Вариант Б — через `.env` (оверрайд без пересборки):
1. Заполнить `AI_SYSTEM_PROMPT` в `.env` доработанным промптом
2. `agent_instruction()` в `context.rs` придётся менять в коде в любом случае

Вариант В — для клиентского приложения:
Если клиент управляет промптом самостоятельно (передаёт его при каждом
запросе), достаточно добавить блок инструкции на русском или английском в
системный промпт на стороне клиента. Серверный промпт не трогается.
