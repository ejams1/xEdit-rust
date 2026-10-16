{ parity probe: x-399 }
unit P_x_399;

function Initialize: integer;
var
  v: Variant;
begin
  if v = 5 then AddMessage('u eq5') else AddMessage('u ne5');
  if v = 0 then AddMessage('u eq0') else AddMessage('u ne0');
  if v <> 5 then AddMessage('u neq5') else AddMessage('u eq5b');
  v := Null;
  if v = 5 then AddMessage('n eq5') else AddMessage('n ne5');
  if v = 0 then AddMessage('n eq0') else AddMessage('n ne0');
  if not (v = 5) then AddMessage('n not-eq5');
  AddMessage('after');
end;
end.
