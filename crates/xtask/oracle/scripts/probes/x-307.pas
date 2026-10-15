{ parity probe: x-307 }
unit P_x_307;

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
  sl.Insert(-1, 'x');
  AddMessage('after');
end;
end.
