package parser

import (
	"testing"

	"github.com/charmbracelet/vhs/lexer"
	"github.com/charmbracelet/vhs/token"
)

// Each literal function-key command must survive tokenization and parsing.
// Native key delivery is proved by the permanent real-TUI PNG scenes: F1 Help,
// F4 Details and F5 Pool details; mapping a key to Enter or Noop cannot satisfy
// those scene oracles.
func TestCaptureFunctionKeys(t *testing.T) {
	for _, key := range []string{"F1", "F4", "F5"} {
		t.Run(key, func(t *testing.T) {
			expectedType := token.Type(key)
			if actual := token.LookupIdentifier(key); actual != expectedType {
				t.Fatalf("%s must be a native key token, got %s", key, actual)
			}
			if !token.IsCommand(expectedType) {
				t.Fatalf("%s must be classified as an executable command", key)
			}
			parser := New(lexer.New(key + "@0ms 2\n"))
			commands := parser.Parse()
			if errors := parser.Errors(); len(errors) != 0 {
				t.Fatalf("%s must parse through the keypress path: %v", key, errors)
			}
			if len(commands) != 1 {
				t.Fatalf("expected one native-key command, got %v", commands)
			}
			command := commands[0]
			if command.Type != CommandType(expectedType) || command.Args != "2" || command.Options != "0ms" {
				t.Fatalf("function-key repetition/speed was lost: %+v", command)
			}
		})
	}
}
