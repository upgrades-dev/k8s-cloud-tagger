{{- define "k8s-cloud-tagger.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
Truncated to 63 characters — Kubernetes name limit.
*/}}
{{- define "k8s-cloud-tagger.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Chart label value (name + version).
*/}}
{{- define "k8s-cloud-tagger.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels applied to every resource.
*/}}
{{- define "k8s-cloud-tagger.labels" -}}
helm.sh/chart: {{ include "k8s-cloud-tagger.chart" . }}
{{ include "k8s-cloud-tagger.selectorLabels" . }}
app.kubernetes.io/version: {{ .Values.image.tag | default .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels — used in Deployment .spec.selector and Service .spec.selector.
Must not change between upgrades.
*/}}
{{- define "k8s-cloud-tagger.selectorLabels" -}}
app.kubernetes.io/name: {{ include "k8s-cloud-tagger.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Azure managed identity resource name.
Uses azure.managedIdentity if set, otherwise falls back to the fullname.
*/}}
{{- define "k8s-cloud-tagger.azure.managedIdentity" -}}
{{- .Values.azure.managedIdentity | default (include "k8s-cloud-tagger.fullname" .) }}
{{- end }}

{{/*
Name of the ConfigMap ASO writes the UserAssignedIdentity principalId into.
Override with azure.serviceOperator.identityConfigMap.
*/}}
{{- define "k8s-cloud-tagger.azure.identityConfigMap" -}}
{{- .Values.azure.serviceOperator.identityConfigMap | default (printf "%s-identity" (include "k8s-cloud-tagger.fullname" .)) }}
{{- end }}

{{/*
Container image reference.
*/}}
{{- define "k8s-cloud-tagger.image" -}}
{{- printf "%s:%s" .Values.image.repository (.Values.image.tag | default .Chart.AppVersion) }}
{{- end }}