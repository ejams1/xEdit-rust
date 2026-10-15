{ parity probe: x-321 }
unit P_x_321;

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
  sl.Free; sl := nil; AddMessage('ok');
  AddMessage('after');
end;
end.
