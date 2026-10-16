{ parity probe: x-400 }
unit P_x_400;

function Process(e: IInterface): integer;
var
  v: Variant;
begin
  Result := 0;
  v := GetElementNativeValues(e, 'NOPE');
  if v = 5 then AddMessage('eq5') else AddMessage('ne5');
  if v <> 0 then AddMessage('ne0') else AddMessage('eq0');
  if v = 0 then AddMessage('eq0b') else AddMessage('ne0b');
end;
end.
