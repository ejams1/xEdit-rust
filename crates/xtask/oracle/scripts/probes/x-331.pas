{ parity probe: x-331 }
unit P_x_331;

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
  v := 5 is TStringList;
  AddMessage('after');
end;
end.
