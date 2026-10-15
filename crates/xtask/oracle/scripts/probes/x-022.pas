{ parity probe: x-022 }
unit P_x_022;

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
  v := nil + 1;
  AddMessage('after');
end;
end.
