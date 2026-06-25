#!/bin/bash

# ANSI colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
GRAY='\033[0;90m'
NC='\033[0m' # No Color

BANNER="${CYAN}  _______                               __  ___           __  _
 /_  __/__  ____  _________  _____     /  |/  /___ ______/ /_  (_)___  ___
  / / / _ \/ __ \/ ___/ __ \/ ___/    / /|_/ / __ \`/ ___/ __ \/ / __ \/ _ \\
 / / /  __/ / / (__  ) /_/ / /       / /  / / /_/ / /__/ / / / / / / /  __/
/_/  \___/_/ /_/____/\____/_/       /_/  /_/\__,_/\___/_/ /_/_/_/ /_/\___/
                                                     Low-Latency RAG Engine${NC}"

init_flatc() {
  if [ -n "${FLATC:-}" ] && [ -x "$FLATC" ]; then
    return
  fi

  local resolver
  resolver="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/tools/flatc/resolve_flatc.sh"
  if [ ! -f "$resolver" ]; then
    echo -e "${RED}flatc resolver not found at $resolver${NC}"
    exit 1
  fi

  FLATC="$(bash "$resolver")"
  export FLATC
}

generate_flatbuffers() {
  local generator
  generator="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/tools/flatc/generate_flatbuffers.sh"
  if [ ! -f "$generator" ]; then
    echo -e "${RED}FlatBuffers generator not found at $generator${NC}"
    exit 1
  fi

  bash "$generator"
}

show_menu() {
  clear
  echo -e "$BANNER"
  echo ""
  echo -e "  1. Install (Cargo Build)"
  echo -e "  2. Start Server (Build + Launch Binary)"
  echo -e "  3. Run Tests (Cargo Test)"
  echo -e "  4. Exit"
  echo ""
  echo -n "Enter your choice [1-4]: "
}

init_flatc

while true; do
  show_menu
  read -r choice

  case $choice in
    1)
      clear
      echo -e "${CYAN}=== Starting Project Build ===${NC}"
      echo -e "${CYAN}=== Generating FlatBuffers Bindings ===${NC}"
      generate_flatbuffers
      cargo build --manifest-path tensor-rag/Cargo.toml --ignore-rust-version
      echo -e "\n${GRAY}Press [Enter] to return to menu...${NC}"
      read -r
      ;;
    2)
      clear
      echo -e "${CYAN}=== Starting RAG Server ===${NC}"
      export TENSOR_RAG_LAUNCHER="true"
      RESTART=true
      while [ "$RESTART" = true ]; do
        cargo build --manifest-path tensor-rag/Cargo.toml --bin tensor-rag --ignore-rust-version
        if [ $? -ne 0 ]; then
          echo -e "\n${RED}Build failed. Press [Enter] to return to menu...${NC}"
          read -r
          RESTART=false
          continue
        fi

        SERVER_BIN="tensor-rag/target/debug/tensor-rag"
        if [ ! -x "$SERVER_BIN" ]; then
          echo -e "\n${RED}Built binary not found at $SERVER_BIN${NC}"
          echo -e "${GRAY}Press [Enter] to return to menu...${NC}"
          read -r
          RESTART=false
          continue
        fi

        echo -e "\n${GREEN}Server is running (waiting for exit/restart)...${NC}"
        "$SERVER_BIN"
        EXIT_CODE=$?

        if [ $EXIT_CODE -eq 123 ]; then
          echo -e "\n${CYAN}Server requested restart (Exit Code 123). Rebuilding and restarting...${NC}"
        else
          RESTART=false
          echo -e "${RED}Server stopped (Exit Code: $EXIT_CODE).${NC}"
          echo -e "\n${GRAY}Press [Enter] to return to menu...${NC}"
          read -r
        fi
      done
      ;;
    3)
      clear
      echo -e "${CYAN}=== Running Test Suite ===${NC}"
      cargo test --manifest-path tensor-rag/Cargo.toml --ignore-rust-version
      echo -e "\n${GRAY}Press [Enter] to return to menu...${NC}"
      read -r
      ;;
    4)
      echo -e "${YELLOW}Goodbye!${NC}"
      exit 0
      ;;
    *)
      echo -e "${RED}Invalid choice! Press [Enter] to try again...${NC}"
      read -r
      ;;
  esac
done
