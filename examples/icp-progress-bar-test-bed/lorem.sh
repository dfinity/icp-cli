#!/bin/bash

# Number of lines to print
LINES=${1:-10}  # default to 10 if not specified

# Some lorem ipsum lines to pick from
LOREM=(
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit."
    "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."
    "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat."
    "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur."
    "Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum."
)

for ((i=1; i<=LINES; i++)); do
    # Pick a random lorem line
    LINE=${LOREM[$RANDOM % ${#LOREM[@]}]}
    
    # Print it
    echo "$LINE"

    # Sleep for a random interval between 0.5 and 3 seconds
    SLEEP_TIME=$(awk -v min=0.1 -v max=1 'BEGIN{srand(); print min+rand()*(max-min)}')
    sleep "$SLEEP_TIME"
done

