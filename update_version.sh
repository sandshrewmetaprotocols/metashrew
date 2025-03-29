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

# Now update dependencies within the project
echo "Updating internal dependencies..."

for CARGO_FILE in $CARGO_FILES; do
    # Get the directory name to identify the crate
    DIR_NAME=$(dirname "$CARGO_FILE")
    CRATE_NAME=$(basename "$DIR_NAME")
    
    # Update dependencies on this crate in all other Cargo.toml files
    for OTHER_CARGO in $CARGO_FILES; do
        if [ "$CARGO_FILE" != "$OTHER_CARGO" ]; then
            # Look for dependencies like: crate_name = { version = "x.y.z", ... }
            # or crate_name = "x.y.z"
            sed -i -E "s/^($CRATE_NAME = \\{ version = ).*/\\1\"$NEW_VERSION\", path = \"..\\/$CRATE_NAME\" \\}/" "$OTHER_CARGO"
            sed -i -E "s/^($CRATE_NAME = ).*/\\1\"$NEW_VERSION\"/" "$OTHER_CARGO"
            
            # Also handle metashrew-specific crates with hyphens that become underscores in dependency names
            UNDERSCORE_NAME=$(echo "$CRATE_NAME" | tr '-' '_')
            if [ "$CRATE_NAME" != "$UNDERSCORE_NAME" ]; then
                sed -i -E "s/^($UNDERSCORE_NAME = \\{ version = ).*/\\1\"$NEW_VERSION\", path = \"..\\/$CRATE_NAME\" \\}/" "$OTHER_CARGO"
                sed -i -E "s/^($UNDERSCORE_NAME = ).*/\\1\"$NEW_VERSION\"/" "$OTHER_CARGO"
            fi
        fi
    done
done

echo "Version update complete!"
