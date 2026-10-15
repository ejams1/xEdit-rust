{ parity probe: x-005 }
unit P_x_005;

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
  d := 1e308 * 10;
  AddMessage('after');
end;
end.
