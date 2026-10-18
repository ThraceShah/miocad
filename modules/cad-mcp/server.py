# Migration notice only. This file never starts CAD or implements a service.
import sys
sys.stderr.write("AI-CAD Python prototype retired. Use Rust cad-mcp. See docs/migration-p4.md.\n")
raise SystemExit(2)
