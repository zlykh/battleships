#!/bin/bash

# Run the command in detached background mode
nohup ~/dev/battleships/target/release/battleship > output.log 2>&1 &

# Get the PID of the last background command
PID=$!

echo $PID > pid

# Print the PID of the detached process
echo "Process started with PID: $PID"
echo "Output will be logged to output.log"