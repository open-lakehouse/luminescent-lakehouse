import { Route, Routes } from "react-router-dom";
import BranchLayout from "./routes/BranchLayout";
import NotFound from "./routes/NotFound";
import WhoAmI from "./routes/WhoAmI";

export default function App() {
  return (
    <Routes>
      <Route path="/b/:endpoint" element={<BranchLayout />}>
        <Route index element={<WhoAmI />} />
      </Route>
      <Route path="*" element={<NotFound />} />
    </Routes>
  );
}
