package yata.rules.eval_policies.bad.fail_not_boolean

import rego.v1

metadata := {
	"title": "Rule contract violation: fail_not_boolean",
	"severity": "low",
	"description": "Gives a string for fail once there is a widget.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

# Without input the first body holds (true); with a widget the else branch gives a string.
fail := true if {
	not input.id
} else := "yes"
