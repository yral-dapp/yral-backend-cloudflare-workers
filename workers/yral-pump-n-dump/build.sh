if [ "$ENV" = "local" ]; then
    echo "MODE: LOCAL"
    cargo install -q worker-build && TEST=1 worker-build --release
else
    echo "MODE: PROD"
    cargo install -q worker-build && worker-build --release
fi
