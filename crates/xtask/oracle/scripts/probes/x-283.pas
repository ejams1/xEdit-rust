{ parity probe: x-283 }
unit P_x_283;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  sl.Add(5); AddMessage(sl[1]);
  AddMessage('after');
end;
end.
