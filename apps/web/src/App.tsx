import { Route, Routes } from "react-router-dom";
import BranchLayout from "./routes/BranchLayout";
import Consent from "./routes/Consent";
import NotFound from "./routes/NotFound";
import SignIn from "./routes/SignIn";
import WhoAmI from "./routes/WhoAmI";

export default function App() {
  return (
    <Routes>
      <Route path="/a/:host/:db" element={<BranchLayout />}>
        <Route index element={<WhoAmI />} />
        <Route path="auth/sign-in" element={<SignIn />} />
        <Route path="auth/consent" element={<Consent />} />
      </Route>
      <Route path="*" element={<NotFound />} />
    </Routes>
  );
}
