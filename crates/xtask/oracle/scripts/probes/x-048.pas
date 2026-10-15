{ parity probe: x-048 }
unit P_x_048;

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
  i := Integer('x');
  AddMessage('after');
end;
end.
