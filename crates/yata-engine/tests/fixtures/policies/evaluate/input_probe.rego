package yata.rules.eval_policies.input_probe

import rego.v1

metadata := {
	"title": "Reports the shape of its input",
	"severity": "info",
	"description": "Fails every widget with details about the input in `actual`.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"related_types": ["aws.example.gadget"],
	"references": [],
}

fail contains finding if {
	some widget in input.aws.example.widget
	finding := {
		"resource_id": widget.id,
		"resource_region": widget.region,
		"message": "probe",
		"actual": {
			"account_id": input.account.id,
			"regions": input.regions,
			"example_keys": sort([k | some k, _ in input.aws.example]),
			"top_keys": sort([k | some k, _ in input]),
			"widget_order": [sprintf("%s@%s", [w.id, w.region]) | some w in input.aws.example.widget],
			"gadget_count": count(input.aws.example.gadget),
		},
	}
}
