default:
    just --list

install:
    cargo install --path .

test:
    cargo nextest run

test-watch:
    fd -e rs | entr -r just test
