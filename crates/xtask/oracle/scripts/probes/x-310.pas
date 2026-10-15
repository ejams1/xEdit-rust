{ parity probe: x-310 }
unit P_x_310;

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
  sl.Delete(-1);
  AddMessage('after');
end;
end.
