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

show_menu() {
  clear
  echo -e "$BANNER"
  echo ""
  echo -e "  1. Install (Cargo Build)"
  echo -e "  2. Start Server (Cargo Run)"
  echo -e "  3. Run Tests (Cargo Test)"
  echo -e "  4. Exit"
  echo ""
  echo -n "Enter your choice [1-4]: "
}

while true; do
  show_menu
  read -r choice

  case $choice in
    1)
      clear
      echo -e "${CYAN}=== Starting Project Build ===${NC}"
      cargo build --manifest-path tensor-rag/Cargo.toml --ignore-rust-version
      echo -e "\n${GRAY}Press [Enter] to return to menu...${NC}"
      read -r
      ;;
    2)
      clear
      echo -e "${CYAN}=== Starting RAG Server ===${NC}"
      cargo run --manifest-path tensor-rag/Cargo.toml --bin tensor-rag --ignore-rust-version &
      SERVER_PID=$!
      echo -e "\n${GREEN}Server is running in background (PID: $SERVER_PID).${NC}"
      echo -e "${YELLOW}Press [Enter] key to stop the server...${NC}"
      read -r
      kill $SERVER_PID
      echo -e "${RED}Server stopped.${NC}"
      echo -e "\n${GRAY}Press [Enter] to return to menu...${NC}"
      read -r
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
