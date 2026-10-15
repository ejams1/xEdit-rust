{ parity probe: x-078 }
unit P_x_078;

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
  v := Chr(65536);
  AddMessage('after');
end;
end.
