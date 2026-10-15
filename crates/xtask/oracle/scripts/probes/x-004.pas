{ parity probe: x-004 }
unit P_x_004;

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
  d := 7.0 / 0;
  AddMessage('after');
end;
end.
