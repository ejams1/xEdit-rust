{ parity probe: x-062 }
unit P_x_062;

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
  v := FileByIndex(99);
  AddMessage('after');
end;
end.
