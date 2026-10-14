package yata.rules.eval_policies.printing

import rego.v1

metadata := {
	"title": "Uses print",
	"severity": "info",
	"description": "Calls print while evaluating.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if {
	print("checking", input.id)
	input.name == "printed"
}

message := "printed"
