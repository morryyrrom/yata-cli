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

fail contains finding if {
	some widget in input.aws.example.widget
	print("checking", widget.id)
	widget.name == "printed"
	finding := {"resource_id": widget.id, "message": "printed"}
}
