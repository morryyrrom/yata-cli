package yata.rules.eval_policies.bad.reason_bad_path

import rego.v1

metadata := {
	"title": "Rule contract violation: reason_bad_path",
	"severity": "low",
	"description": "Gives a path that is not a JSON Pointer when the widget is named bad.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.name == "bad"

reason := {"path": "no-slash", "actual": 1}
