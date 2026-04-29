/**
 * CloudWatch (Metrics + Alarms + Dashboards) API client.
 *
 * Wraps the AWSim monitoring query-protocol endpoints
 * (Action=… form-encoded XML responses). Every result is normalised to
 * a small camel-cased object so the UI does not have to parse XML.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/monitoring/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function monitoringQuery(
  action: string,
  params: Record<string, string> = {},
): Promise<Document> {
  const body = new URLSearchParams({
    Action: action,
    Version: "2010-08-01",
    ...params,
  });
  const res = await fetch(`${ENDPOINT}/`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: body.toString(),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  }
  const text = await res.text();
  return new DOMParser().parseFromString(text, "application/xml");
}

function txt(el: Element | null, tag: string): string {
  return el?.querySelector(tag)?.textContent ?? "";
}

function num(el: Element | null, tag: string): number {
  const t = txt(el, tag);
  return t ? parseFloat(t) : 0;
}

// -- Types --

export interface MetricDimension {
  name: string;
  value: string;
}

export interface Metric {
  namespace: string;
  metricName: string;
  dimensions: MetricDimension[];
}

export interface MetricDatapoint {
  timestamp: number;
  average?: number;
  sum?: number;
  minimum?: number;
  maximum?: number;
  sampleCount?: number;
  unit?: string;
}

export interface Alarm {
  alarmName: string;
  alarmDescription?: string;
  namespace: string;
  metricName: string;
  stateValue: string;
  stateReason?: string;
  stateUpdatedTimestamp?: string;
  threshold: number;
  comparisonOperator: string;
  period: number;
  evaluationPeriods: number;
  statistic?: string;
  unit?: string;
  dimensions: Array<{ name: string; value: string }>;
}

export interface DashboardSummary {
  name: string;
  arn?: string;
  lastModified?: string;
  size?: number;
}

export interface DashboardDetail extends DashboardSummary {
  body: string;
}

// -- Operations --

export async function listMetrics(): Promise<{ metrics: Metric[] }> {
  const doc = await monitoringQuery("ListMetrics");
  const members = Array.from(
    doc.querySelectorAll("ListMetricsResult > Metrics > member"),
  );
  const metrics: Metric[] = members.map((el) => {
    const dims = Array.from(el.querySelectorAll("Dimensions > member")).map(
      (d) => ({
        name: txt(d, "Name"),
        value: txt(d, "Value"),
      }),
    );
    return {
      namespace: txt(el, "Namespace"),
      metricName: txt(el, "MetricName"),
      dimensions: dims,
    };
  });
  return { metrics: metrics.filter((m) => m.metricName !== "") };
}

export async function getMetricStatistics(
  namespace: string,
  metricName: string,
  startSecs: number,
  endSecs: number,
  periodSecs = 60,
  statistic:
    | "Average"
    | "Sum"
    | "Minimum"
    | "Maximum"
    | "SampleCount" = "Average",
  dimensions: MetricDimension[] = [],
): Promise<{ datapoints: MetricDatapoint[] }> {
  const params: Record<string, string> = {
    Namespace: namespace,
    MetricName: metricName,
    StartTime: new Date(startSecs * 1000).toISOString(),
    EndTime: new Date(endSecs * 1000).toISOString(),
    Period: String(periodSecs),
    "Statistics.member.1": statistic,
  };
  dimensions.forEach((d, i) => {
    params[`Dimensions.member.${i + 1}.Name`] = d.name;
    params[`Dimensions.member.${i + 1}.Value`] = d.value;
  });
  const doc = await monitoringQuery("GetMetricStatistics", params);
  const points = Array.from(
    doc.querySelectorAll("GetMetricStatisticsResult > Datapoints > member"),
  ).map<MetricDatapoint>((el) => ({
    timestamp: Date.parse(txt(el, "Timestamp")) || 0,
    average: el.querySelector("Average") ? num(el, "Average") : undefined,
    sum: el.querySelector("Sum") ? num(el, "Sum") : undefined,
    minimum: el.querySelector("Minimum") ? num(el, "Minimum") : undefined,
    maximum: el.querySelector("Maximum") ? num(el, "Maximum") : undefined,
    sampleCount: el.querySelector("SampleCount")
      ? num(el, "SampleCount")
      : undefined,
    unit: txt(el, "Unit") || undefined,
  }));
  points.sort((a, b) => a.timestamp - b.timestamp);
  return { datapoints: points };
}

export async function describeAlarms(): Promise<{ alarms: Alarm[] }> {
  const doc = await monitoringQuery("DescribeAlarms");
  const members = Array.from(
    doc.querySelectorAll("DescribeAlarmsResult > MetricAlarms > member"),
  );
  const alarms: Alarm[] = members.map((el) => {
    const dims = Array.from(el.querySelectorAll("Dimensions > member")).map(
      (d) => ({
        name: d.querySelector("Name")?.textContent ?? "",
        value: d.querySelector("Value")?.textContent ?? "",
      }),
    );
    return {
      alarmName: txt(el, "AlarmName"),
      alarmDescription: txt(el, "AlarmDescription") || undefined,
      namespace: txt(el, "Namespace"),
      metricName: txt(el, "MetricName"),
      stateValue: txt(el, "StateValue"),
      stateReason: txt(el, "StateReason") || undefined,
      stateUpdatedTimestamp: txt(el, "StateUpdatedTimestamp") || undefined,
      threshold: num(el, "Threshold"),
      comparisonOperator: txt(el, "ComparisonOperator"),
      period: num(el, "Period") || 60,
      evaluationPeriods: num(el, "EvaluationPeriods") || 1,
      statistic: txt(el, "Statistic") || undefined,
      unit: txt(el, "Unit") || undefined,
      dimensions: dims.filter((d) => d.name !== ""),
    };
  });
  return { alarms: alarms.filter((a) => a.alarmName !== "") };
}

export async function listDashboards(): Promise<{
  dashboards: DashboardSummary[];
}> {
  const doc = await monitoringQuery("ListDashboards");
  const members = Array.from(
    doc.querySelectorAll("ListDashboardsResult > DashboardEntries > member"),
  );
  return {
    dashboards: members.map((el) => ({
      name: txt(el, "DashboardName"),
      arn: txt(el, "DashboardArn") || undefined,
      lastModified: txt(el, "LastModified") || undefined,
      size: el.querySelector("Size") ? num(el, "Size") : undefined,
    })),
  };
}

export async function getDashboard(name: string): Promise<DashboardDetail> {
  const doc = await monitoringQuery("GetDashboard", { DashboardName: name });
  const root = doc.querySelector("GetDashboardResult");
  return {
    name: txt(root, "DashboardName") || name,
    arn: txt(root, "DashboardArn") || undefined,
    body: txt(root, "DashboardBody") || "",
  };
}
