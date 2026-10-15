{ parity probe: x-061 }
unit P_x_061;

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
  v := sl.Count(1);
  AddMessage('after');
end;
end.
