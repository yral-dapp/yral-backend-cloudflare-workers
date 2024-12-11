if [ "$ENV" = "local" ]; then
    echo "MODE: LOCAL"
    cargo install -q worker-build && worker-build --release -- --features local
else
    echo "MODE: PROD"
    cargo install -q worker-build && worker-build --release
fi
