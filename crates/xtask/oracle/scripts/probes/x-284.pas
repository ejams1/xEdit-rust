{ parity probe: x-284 }
unit P_x_284;

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
  sl.Add(1.5); AddMessage(sl[1]);
  AddMessage('after');
end;
end.
