{ parity probe: x-300 }
unit P_x_300;

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
  sl.Sorted := 5; AddMessage('ok');
  AddMessage('after');
end;
end.
