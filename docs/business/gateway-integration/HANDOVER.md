# OCLA Gateway-Integration — Handover

Stand: `main` bei `d7f8324b7` (`#1093`, MetricsExporter-Produktionspfad).

## Definition des Status

„Verdrahtet“ bedeutet hier: Der Builtin wird über
`OclaRegistry::global()` aus einem produktiven Laufzeitpfad aufgerufen.
„Registry-only“ bedeutet: Die Trait-Implementierung ist real und getestet,
aber noch nicht an einen produktiven Aufrufer angeschlossen. Registry-only
ist kein `unimplemented!()`-Stub; es fehlt ausschließlich die Adoption.

## Builtin-Inventar

| Builtin | Status | Aktueller Produktionspfad / Lücke |
| --- | --- | --- |
| `AgentGateway` | Registry-only | Envelope-Validierung und Relay vorhanden; A2A-Ingress nutzt den Builtin noch nicht. |
| `CompressionProvider` | Verdrahtet | Aggressive `ctx_read`-Kompression in `core/tool_lifecycle`; ContentPort mit PathJail und BLAKE3-Referenz. |
| `ConfigTuner` | Registry-only | Proposal-/Approval-Semantik vorhanden; kein produktiver OCLA-Tuning-Aufrufer. |
| `ConnectorScheduler` | Registry-only | Begrenzte In-Memory-Queue vorhanden; kein produktiver Connector-Dispatch. |
| `EfficiencyAnalyzer` | Verdrahtet | `core/tool_lifecycle` berechnet Read-Density und ETPAO über den OCLA-Trait. |
| `ExperimentRunner` | Registry-only | Deterministische Ergebnis-/Rollback-Referenzen vorhanden; kein Live-Holdout-Aufrufer. |
| `IntentClassifier` | Registry-only | Kandidatenbasierte Klassifikation vorhanden; kein produktiver OCLA-Aufruf. |
| `MetricsExporter` | Verdrahtet | `tools/server_metrics` exportiert pro MCP-Call ein begrenztes lokales Batch (`#1093`). |
| `ModelRouter` | Registry-only | Deterministisches OCLA-Routing vorhanden; bestehender Proxy-Router ist noch nicht auf den Builtin umgestellt. |
| `ObservationHook` | Verdrahtet | `tools/server_metrics` projiziert jeden MCP-Tool-Call als Observation. |
| `OutcomeTracker` | Verdrahtet | `tools/server_metrics` schreibt Accepted-/Quality-Ergebnis nach jedem MCP-Call. |
| `ResponseOptimizer` | Registry-only | Trait-Wrapper und deterministische Optimierung vorhanden; bestehender Proxy-Optimizer bleibt separat. |
| `SavingsLedger` | Registry-only | OCLA-Evidence-Ledger vorhanden; produktive Abrechnung nutzt weiterhin den bestehenden Core-Ledger. |
| `UsageSink` | Verdrahtet | `proxy/usage_meter` projiziert den finalisierten Provider-Turn in den OCLA-Sink. |

Damit sind 6 Builtins produktiv adoptiert und 8 Builtins für die nächste
Adoptionsrunde offen.

## Gemergter Stand auf `main`

- `#1053`: P1 Foundation, Ledger Evidence, Contracts und Proxy Lineage.
- `#1065`: alle 14 Trait-Implementierungen und `OclaRegistry`.
- `#1070`: UsageSink- und EfficiencyAnalyzer-Produktionsaufrufe.
- `#1071`: ObservationHook am MCP-Tool-Call-Boundary.
- `#1073`: OutcomeTracker am MCP-Tool-Call-Boundary.
- `#1075`, `#1076`, `#1083`, `#1092`: sicherer ContentPort und echte,
  fail-closed CompressionProvider-Verdrahtung.
- `#1093`: MetricsExporter-Produktionsaufruf in `server_metrics`.

## Aktive Arbeit und nächste Schritte

Diese Dokumentation ist der SSOT-Sync von Agent 19. Der Review-/Merge-Agent
20 bleibt für Push und Merge zuständig; ungemergte Arbeitsstände gehören nicht
zum Stand auf `main`.

Nächste Runtime-Adoptionen, in dieser Reihenfolge:

1. `AgentGateway`, `ModelRouter`, `ResponseOptimizer` und `SavingsLedger`
   an ihre bestehenden Produktionsgrenzen anschließen.
2. `ConfigTuner`, `ConnectorScheduler`, `ExperimentRunner` und
   `IntentClassifier` an echte Aufrufer anschließen.
3. Pro neuem Pfad Boundary-, Fehler- und No-feedback-in-legacy-Tests ergänzen.

## OSS/Private-Boundary-Audit

Der vorgeschriebene Suchlauf

```text
grep -rn 'enterprise\|Enterprise\|RBAC\|SSO\|multi.tenant\|value.gate' rust/src/ --include='*.rs'
```

liefert Treffer in `rust/src`. Beispiele sind Enterprise-markierte Kommentare
und Module rund um Deployment-Profile, Billing, Policy-Gate, SSO und Gateway-
Server. Das ist kein sauberer OSS-Audit: Der Befund wurde als Blocker an den
Agent-Bus gemeldet (Event `c3f76d6b`). Agent 20 bzw. der Boundary-Owner muss
vor dem Merge klären, welche Treffer entfernt oder in das Private-Repo
verschoben werden.

README.md und VISION.md enthalten keine widersprüchliche OCLA-Behauptung;
beide beschreiben weiterhin die lokale, provider-neutrale Architektur. Eine
Änderung dieser Dateien ist für den aktuellen Runtime-Stand nicht erforderlich.
