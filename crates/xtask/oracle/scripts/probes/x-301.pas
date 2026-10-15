{ parity probe: x-301 }
unit P_x_301;

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
  sl.Sorted := 'x';
  AddMessage('after');
end;
end.
