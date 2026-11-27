import { Button } from "@cascivo/react";
import { useState } from "react";

export function Hello() {
  const [count, setCount] = useState(0);
  return <Button onClick={() => setCount(count + 1)}>Clicked {count} times</Button>;
}
