#!/bin/bash

# Check if a version argument was provided
if [ $# -ne 1 ]; then
    echo "Usage: $0 <new_version>"
    echo "Example: $0 0.2.0"
    exit 1
fi

NEW_VERSION=$1

# Validate version format (basic check for semver format)
if ! [[ $NEW_VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9\.]+)?(\+[a-zA-Z0-9\.]+)?$ ]]; then
    echo "Error: Version must be in semver format (e.g., 1.2.3, 1.2.3-alpha, 1.2.3+build.1)"
    exit 1
fi

echo "Updating all crates to version $NEW_VERSION"

# Find all Cargo.toml files in the repository
CARGO_FILES=$(find . -name "Cargo.toml" -type f)

# Counter for updated files
UPDATED=0

for CARGO_FILE in $CARGO_FILES; do
    # Skip the workspace Cargo.toml if it exists
    if grep -q '^\[workspace\]' "$CARGO_FILE"; then
        echo "Skipping workspace file: $CARGO_FILE"
        continue
    fi
    
    # Check if this is a package Cargo.toml (has [package] section)
    if grep -q '^\[package\]' "$CARGO_FILE"; then
        # Update the version using sed
        # This replaces the line starting with "version = " in the [package] section
        if sed -i "/^\[package\]/,/^\[.*\]/ s/^version = .*/version = \"$NEW_VERSION\"/" "$CARGO_FILE"; then
            echo "Updated: $CARGO_FILE"
            UPDATED=$((UPDATED + 1))
        else
            echo "Failed to update: $CARGO_FILE"
        fi
    else
        echo "Skipping non-package file: $CARGO_FILE"
    fi
done

echo "Updated $UPDATED Cargo.toml files to version $NEW_VERSION"

# We're not updating internal dependencies to avoid breaking path references
echo "Skipping internal dependency updates to preserve path references"

# The script now only updates the version field in each crate's Cargo.toml
# and does not modify dependency specifications between sibling crates

echo "Version update complete!"
