# Converts all leading spaces in recipe lines (after a target:) to a single tab, as required by Makefile syntax.

set -e
#!/bin/bash
# fix_makefile_tabs.sh
# Usage: ./fix_makefile_tabs.sh Makefile
# Replaces all leading spaces with a single tab for shell command lines in Makefile targets.

set -e

if [ -z "$1" ]; then
  echo "Usage: $0 Makefile"
  exit 1
fi

awk '
  BEGIN { inrecipe=0 }
  /^[^ \t#].*:/ { inrecipe=1; print; next }
  /^[ \t]*$/ { inrecipe=0; print; next }
  {
    if (inrecipe && $0 ~ /^[ ]+/) {
      sub(/^[ ]+/, "\t");
      print
    } else {
      print
    }
  }
' "$1" > "$1.fixed"
echo "Fixed Makefile written to $1.fixed"
echo "Review and replace your Makefile if satisfied: mv $1.fixed $1"
