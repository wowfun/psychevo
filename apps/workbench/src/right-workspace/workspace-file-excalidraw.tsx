import type {
  ExcalidrawDocument,
  ExcalidrawElement
} from "./workspace-file-excalidraw-data";

export type { ExcalidrawDocument } from "./workspace-file-excalidraw-data";
export {
  readExcalidrawDocument,
  workspaceExcalidrawPolicy
} from "./workspace-file-excalidraw-data";

export function ExcalidrawPreview({
  document,
  path
}: {
  document: ExcalidrawDocument;
  path: string;
}) {
  return (
    <svg
      aria-label={`Preview ${path}`}
      className="workspaceExcalidrawPreview"
      preserveAspectRatio="xMidYMid meet"
      role="img"
      viewBox={document.viewBox}
    >
      {document.elements.map((element) => (
        <g
          key={element.id}
          transform={`rotate(${element.angle * 180 / Math.PI} ${element.x + element.width / 2} ${element.y + element.height / 2})`}
        >
          <ExcalidrawShape element={element} />
        </g>
      ))}
    </svg>
  );
}

function ExcalidrawShape({ element }: { element: ExcalidrawElement }) {
  if (element.type === "ellipse") {
    return (
      <ellipse
        cx={element.x + element.width / 2}
        cy={element.y + element.height / 2}
        fill={element.backgroundColor}
        rx={Math.abs(element.width / 2)}
        ry={Math.abs(element.height / 2)}
        stroke={element.strokeColor}
      />
    );
  }
  if (element.type === "diamond") {
    const centerX = element.x + element.width / 2;
    const centerY = element.y + element.height / 2;
    return (
      <polygon
        fill={element.backgroundColor}
        points={`${centerX},${element.y} ${element.x + element.width},${centerY} ${centerX},${element.y + element.height} ${element.x},${centerY}`}
        stroke={element.strokeColor}
      />
    );
  }
  if (element.type === "text" || element.text !== null) {
    return (
      <text fill={element.strokeColor} fontSize={16} x={element.x} y={element.y + 18}>
        {(element.text ?? "").split("\n").map((line, index) => (
          <tspan key={index} dy={index === 0 ? 0 : 20} x={element.x}>{line}</tspan>
        ))}
      </text>
    );
  }
  if (element.type === "line" || element.type === "arrow") {
    return (
      <line
        stroke={element.strokeColor}
        x1={element.x}
        x2={element.x + element.width}
        y1={element.y}
        y2={element.y + element.height}
      />
    );
  }
  return (
    <rect
      fill={element.backgroundColor}
      height={Math.abs(element.height)}
      rx={4}
      stroke={element.strokeColor}
      width={Math.abs(element.width)}
      x={element.x}
      y={element.y}
    />
  );
}
