@echo off
set RUST_LOG=info,tensor_rag=debug
cd /d F:\projects\APL-RAG
start /b F:\projects\APL-RAG\tensor-rag\target\debug\tensor-rag.exe >> server.log 2>&1
