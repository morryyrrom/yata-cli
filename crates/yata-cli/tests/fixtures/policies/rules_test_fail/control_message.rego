package yata.rules.cli_fail_policies.control_message

import rego.v1

metadata := {
	"title": "Always fails with control characters in the message",
	"severity": "low",
	"description": "The message contains an ANSI escape sequence and a line break.",
	"remediation": "None.",
	"target_type": "aws.account.account",
	"references": [],
}

# The control characters are written as escapes so that this file has none of its own.
fail contains finding if {
	some r in input.aws.account.account
	finding := {"resource_id": r.id, "message": "red \u001b[31mtext\u001b[0m\nsecond line"}
}
